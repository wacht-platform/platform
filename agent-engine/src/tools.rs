mod code_runner;
mod external;
mod internal;
mod platform;
mod result_shape;

use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use common::error::AppError;
use common::state::AppState;
use dto::json::{agent_executor::ToolCallRequest, StreamEvent};
use models::AiAgentWithFeatures;
use models::{AiTool, AiToolConfiguration};
use rand::Rng;
use serde::Serialize;
use serde_json::Value;

pub struct ToolExecutor {
    ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    channel: Option<tokio::sync::mpsc::Sender<StreamEvent>>,
    active_board_item_id: Option<i64>,
}

const DEFAULT_INLINE_OUTPUT_THRESHOLD_CHARS: usize = 60_000;
const WEB_CONTEXT_INLINE_OUTPUT_THRESHOLD_CHARS: usize = 200_000;

impl ToolExecutor {
    pub fn new(
        ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    ) -> Self {
        Self {
            ctx,
            channel: None,
            active_board_item_id: None,
        }
    }

    pub fn with_channel(mut self, channel: tokio::sync::mpsc::Sender<StreamEvent>) -> Self {
        self.channel = Some(channel);
        self
    }

    pub fn set_active_board_item_id(&mut self, board_item_id: Option<i64>) {
        self.active_board_item_id = board_item_id;
    }

    #[inline]
    pub(crate) fn active_board_item_id(&self) -> Option<i64> {
        self.active_board_item_id
    }

    #[inline]
    fn app_state(&self) -> &AppState {
        &self.ctx.app_state
    }

    #[inline]
    fn agent(&self) -> &AiAgentWithFeatures {
        &self.ctx.agent
    }

    #[inline]
    fn thread_id(&self) -> i64 {
        self.ctx.thread_id
    }

    fn serialize_tool_output<T: Serialize>(&self, result: T) -> Result<Value, AppError> {
        serde_json::to_value(result).map_err(AppError::from)
    }

    async fn execute_tool_from_input(
        &self,
        tool: &AiTool,
        execution_params: Value,
        filesystem: &AgentFilesystem,
        _shell: &ShellExecutor,
        context_title: &str,
    ) -> Result<Value, AppError> {
        let mut final_result = match &tool.configuration {
            AiToolConfiguration::Api(config) => {
                let result = self
                    .execute_api_tool(tool, config, &execution_params)
                    .await?;
                self.serialize_tool_output(result)?
            }
            AiToolConfiguration::PlatformEvent(config) => {
                let result = self
                    .execute_platform_event_tool(tool, config, &execution_params)
                    .await?;
                self.serialize_tool_output(result)?
            }
            AiToolConfiguration::CodeRunner(config) => {
                let result = self
                    .execute_code_runner_tool(tool, config, &execution_params, filesystem)
                    .await?;
                self.serialize_tool_output(result)?
            }
            AiToolConfiguration::Internal(_config) => {
                return Err(AppError::Internal(
                    "Internal tools must execute from structured tool requests".to_string(),
                ));
            }
            AiToolConfiguration::UseExternalService(config) => {
                self.execute_external_service_tool(
                    tool,
                    config,
                    &execution_params,
                    context_title,
                    filesystem,
                )
                .await?
            }
        };

        if final_result.is_object() && final_result.get("structure_hint").is_none() {
            let mut special_hint = None;
            for key in ["data", "result", "stdout"] {
                if let Some(val) = final_result.get(key) {
                    if let Some(s) = val.as_str() {
                        if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                            special_hint = Some(format!(
                                "(key '{}' contains parsed JSON) {}",
                                key,
                                result_shape::infer_schema_hint(&parsed)
                            ));
                            break;
                        }
                    }
                }
            }

            let hint =
                special_hint.unwrap_or_else(|| result_shape::infer_schema_hint(&final_result));

            if let Some(obj) = final_result.as_object_mut() {
                obj.insert("structure_hint".to_string(), serde_json::json!(hint));
            }
        }

        let result_str = serde_json::to_string_pretty(&final_result)?;
        let char_count = result_str.chars().count();
        let threshold = match tool.name.as_str() {
            "web_search" | "url_content" => WEB_CONTEXT_INLINE_OUTPUT_THRESHOLD_CHARS,
            _ => DEFAULT_INLINE_OUTPUT_THRESHOLD_CHARS,
        };

        let timestamp = chrono::Utc::now().timestamp_millis();
        let random_suffix: String = (0..4)
            .map(|_| {
                let idx = rand::thread_rng().gen_range(0..36);
                const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
                CHARS[idx] as char
            })
            .collect();

        let scratch_filename = format!("tool_output_{}_{}.txt", timestamp, random_suffix);
        let scratch_path = format!("scratch/{}", scratch_filename);
        let scratch_write_result = filesystem
            .write_file(&scratch_path, &result_str, false)
            .await;
        let scratch_saved = scratch_write_result.is_ok();
        let scratch_write_error = scratch_write_result.err().map(|e| e.to_string());

        let lines = result_str.lines().count();
        let size_bytes = result_str.len();

        if char_count > threshold {
            let structure_hint = final_result
                .get("structure_hint")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            return Ok(serde_json::json!({
                "truncated": true,
                "data_omitted": true,
                "structure_hint": structure_hint,
                "original_stats": {
                    "size_bytes": size_bytes,
                    "lines": lines,
                    "char_count": char_count,
                    "saved_to_path": if scratch_saved { serde_json::json!(scratch_path) } else { serde_json::Value::Null }
                },
                "persistence_error": scratch_write_error,
                "hint": format!(
                    "{}",
                    if scratch_saved {
                        format!(
                    "Output is larger than {} chars, so inline data is omitted. Read '{}' now (execution-scoped temp file) and filter with execute_command.",
                            threshold, scratch_path
                        )
                    } else {
                        format!(
                            "Output is larger than {} chars, so inline data is omitted. Could not persist a scratch copy due to a write error.",
                            threshold
                        )
                    }
                ),
            }));
        }

        if let Some(obj) = final_result.as_object_mut() {
            if scratch_saved {
                obj.insert(
                    "saved_output_path".to_string(),
                    serde_json::json!(scratch_path),
                );
            } else if let Some(error) = scratch_write_error.as_ref() {
                obj.insert("persistence_error".to_string(), serde_json::json!(error));
            }
            obj.insert(
                "output_notice".to_string(),
                serde_json::json!(if scratch_saved {
                    "Output is shown inline once and saved as an execution-scoped temp file."
                } else {
                    "Output is shown inline, but the execution-scoped temp file could not be persisted."
                }),
            );
        } else {
            let mut payload = serde_json::json!({
                "result": final_result,
                "output_notice": if scratch_saved {
                    "Output is shown inline once and saved as an execution-scoped temp file."
                } else {
                    "Output is shown inline, but the execution-scoped temp file could not be persisted."
                }
            });
            if let Some(obj) = payload.as_object_mut() {
                if scratch_saved {
                    obj.insert(
                        "saved_output_path".to_string(),
                        serde_json::json!(scratch_path),
                    );
                } else if let Some(error) = scratch_write_error.as_ref() {
                    obj.insert("persistence_error".to_string(), serde_json::json!(error));
                }
            }
            final_result = payload;
        }

        Ok(final_result)
    }

    pub async fn execute_tool_request(
        &self,
        tool: &AiTool,
        request: &ToolCallRequest,
        filesystem: &AgentFilesystem,
        shell: &ShellExecutor,
        context_title: &str,
    ) -> Result<Value, AppError> {
        match request {
            ToolCallRequest::External(_) => {
                let params = request.input_value().map_err(|e| {
                    AppError::Internal(format!("Failed to serialize tool input: {e}"))
                })?;
                self.execute_tool_from_input(tool, params, filesystem, shell, context_title)
                    .await
            }
            _ => {
                self.execute_internal_tool_request(tool, request, filesystem, shell)
                    .await
            }
        }
    }
}
