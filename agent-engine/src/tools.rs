pub(crate) mod approval;
mod code_runner;
pub mod external;
mod internal;
pub mod internal_specs;
pub(crate) mod mcp;
mod platform;
mod result_shape;
pub mod system_skills;

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
    sandbox_handle: Option<std::sync::Arc<dyn crate::sandbox::SandboxHandle>>,
}

const INLINE_OUTPUT_THRESHOLD_CHARS: usize = 60_000;
const COMPLEXITY_GATE_MIN_CHARS: usize = 2_000;
const COMPLEXITY_MAX_DEPTH: usize = 5;
const COMPLEXITY_MAX_LEAVES: usize = 150;
const COMPLEXITY_MAX_OBJECT_ARRAY_LEN: usize = 20;

fn compute_shape_hint(value: &Value) -> String {
    if let Some(obj) = value.as_object() {
        for key in ["data", "result", "stdout"] {
            if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    return format!(
                        "(key '{}' contains parsed JSON) {}",
                        key,
                        result_shape::infer_schema_hint(&parsed)
                    );
                }
            }
        }
    }
    result_shape::infer_schema_hint(value)
}

impl ToolExecutor {
    pub fn new(
        ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    ) -> Self {
        Self {
            ctx,
            channel: None,
            active_board_item_id: None,
            sandbox_handle: None,
        }
    }

    pub fn with_channel(mut self, channel: tokio::sync::mpsc::Sender<StreamEvent>) -> Self {
        self.channel = Some(channel);
        self
    }

    pub fn with_sandbox_handle(
        mut self,
        handle: std::sync::Arc<dyn crate::sandbox::SandboxHandle>,
    ) -> Self {
        self.sandbox_handle = Some(handle);
        self
    }

    pub(crate) fn sandbox_handle(
        &self,
    ) -> Result<std::sync::Arc<dyn crate::sandbox::SandboxHandle>, AppError> {
        self.sandbox_handle
            .clone()
            .ok_or_else(|| AppError::Internal("sandbox handle is not configured".into()))
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

    #[tracing::instrument(
        name = "tool.execute_from_input",
        skip(self, tool, execution_params, filesystem, _shell),
        fields(
            tool_id = tool.id,
            tool_name = %tool.name,
            thread_id = self.thread_id(),
            deployment_id = self.agent().deployment_id,
        ),
    )]
    async fn execute_tool_from_input(
        &self,
        tool: &AiTool,
        execution_params: Value,
        filesystem: &AgentFilesystem,
        _shell: &ShellExecutor,
    ) -> Result<Value, AppError> {
        let final_result = match &tool.configuration {
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
            AiToolConfiguration::Mcp(config) => {
                self.execute_mcp_tool(tool, config, &execution_params, filesystem)
                    .await?
            }
            AiToolConfiguration::Virtual(config) => {
                let thread = self.ctx.get_thread().await?;
                external::execute_external_tool(
                    &self.ctx.app_state,
                    self.ctx.agent.deployment_id,
                    thread.actor_id,
                    config,
                    &execution_params,
                )
                .await?
            }
        };

        self.apply_output_postprocess(final_result, filesystem)
            .await
    }

    async fn apply_output_postprocess(
        &self,
        mut final_result: Value,
        filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        let hint = compute_shape_hint(&final_result);

        let result_str = serde_json::to_string_pretty(&final_result)?;
        let char_count = result_str.chars().count();
        let threshold = INLINE_OUTPUT_THRESHOLD_CHARS;

        let complexity = result_shape::complexity_metrics(&final_result);
        let too_complex_for_inline = char_count >= COMPLEXITY_GATE_MIN_CHARS
            && (complexity.max_depth > COMPLEXITY_MAX_DEPTH
                || complexity.leaf_count > COMPLEXITY_MAX_LEAVES
                || complexity.max_object_array_len > COMPLEXITY_MAX_OBJECT_ARRAY_LEN);

        let timestamp = chrono::Utc::now().timestamp_millis();
        let random_suffix: String = (0..4)
            .map(|_| {
                let idx = rand::thread_rng().gen_range(0..36);
                const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
                CHARS[idx] as char
            })
            .collect();

        let scratch_filename = format!("tool_output_{}_{}.txt", timestamp, random_suffix);
        let scratch_path = format!("/scratch/{}", scratch_filename);
        let scratch_write_result = filesystem
            .write_file(&scratch_path, &result_str, false)
            .await;
        let scratch_saved = scratch_write_result.is_ok();
        let scratch_write_error = scratch_write_result.err().map(|e| e.to_string());

        let lines = result_str.lines().count();
        let size_bytes = result_str.len();

        if char_count > threshold || too_complex_for_inline {
            let reason = if char_count > threshold {
                format!("Output is larger than {} chars", threshold)
            } else {
                format!(
                    "Output is structurally complex (depth={}, leaves={}, max_object_array_len={})",
                    complexity.max_depth, complexity.leaf_count, complexity.max_object_array_len
                )
            };

            return Ok(serde_json::json!({
                "truncated": true,
                "data_omitted": true,
                "saved_output_shape": hint,
                "original_stats": {
                    "size_bytes": size_bytes,
                    "lines": lines,
                    "char_count": char_count,
                    "max_depth": complexity.max_depth,
                    "leaf_count": complexity.leaf_count,
                    "max_object_array_len": complexity.max_object_array_len,
                    "saved_to_path": if scratch_saved { serde_json::json!(scratch_path) } else { serde_json::Value::Null }
                },
                "persistence_error": scratch_write_error,
                "hint": if scratch_saved {
                    format!(
                        "{}, so inline data is omitted. Read '{}' now (execution-scoped temp file) and filter with execute_command.",
                        reason, scratch_path
                    )
                } else {
                    format!(
                        "{}, so inline data is omitted. Could not persist a scratch copy due to a write error.",
                        reason
                    )
                },
            }));
        }

        if let Some(obj) = final_result.as_object_mut() {
            if scratch_saved {
                obj.insert(
                    "saved_output_path".to_string(),
                    serde_json::json!(scratch_path),
                );
                obj.insert("saved_output_shape".to_string(), serde_json::json!(hint));
            } else if let Some(error) = scratch_write_error.as_ref() {
                obj.insert("persistence_error".to_string(), serde_json::json!(error));
            }
        } else {
            let mut payload = serde_json::json!({
                "result": final_result,
            });
            if let Some(obj) = payload.as_object_mut() {
                if scratch_saved {
                    obj.insert(
                        "saved_output_path".to_string(),
                        serde_json::json!(scratch_path),
                    );
                    obj.insert("saved_output_shape".to_string(), serde_json::json!(hint));
                } else if let Some(error) = scratch_write_error.as_ref() {
                    obj.insert("persistence_error".to_string(), serde_json::json!(error));
                }
            }
            final_result = payload;
        }

        Ok(final_result)
    }

    #[tracing::instrument(
        name = "tool.execute_request",
        skip(self, tool, request, filesystem, shell),
        fields(
            tool_id = tool.id,
            tool_name = %tool.name,
            thread_id = self.thread_id(),
            deployment_id = self.agent().deployment_id,
        ),
    )]
    pub async fn execute_tool_request(
        &self,
        tool: &AiTool,
        request: &ToolCallRequest,
        filesystem: &AgentFilesystem,
        shell: &ShellExecutor,
    ) -> Result<Value, AppError> {
        match request {
            ToolCallRequest::External(_) => {
                let params = request.input_value().map_err(|e| {
                    AppError::Internal(format!("Failed to serialize tool input: {e}"))
                })?;
                self.execute_tool_from_input(tool, params, filesystem, shell)
                    .await
            }
            _ => {
                let result = self
                    .execute_internal_tool_request(tool, request, filesystem, shell)
                    .await?;
                self.apply_output_postprocess(result, filesystem).await
            }
        }
    }
}
