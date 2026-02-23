mod external;
mod internal;
mod platform;
mod result_shape;

use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use common::error::AppError;
use common::state::AppState;
use dto::json::StreamEvent;
use models::AiAgentWithFeatures;
use models::{AiTool, AiToolConfiguration};
use rand::Rng;
use serde_json::Value;

pub struct ToolExecutor {
    ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
    channel: Option<tokio::sync::mpsc::Sender<StreamEvent>>,
}

impl ToolExecutor {
    pub fn new(ctx: std::sync::Arc<crate::execution_context::ExecutionContext>) -> Self {
        Self { ctx, channel: None }
    }

    pub fn with_channel(mut self, channel: tokio::sync::mpsc::Sender<StreamEvent>) -> Self {
        self.channel = Some(channel);
        self
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
    fn context_id(&self) -> i64 {
        self.ctx.context_id
    }

    async fn create_lite_llm(&self) -> crate::GeminiClient {
        self.ctx
            .create_llm("gemini-2.5-flash-lite")
            .await
            .unwrap_or_else(|_| {
                let api_key = std::env::var("GEMINI_API_KEY").unwrap();
                crate::GeminiClient::new(api_key, "gemini-2.5-flash-lite".to_string())
                    .with_billing(
                        self.agent().deployment_id,
                        self.app_state().redis_client.clone(),
                    )
                    .with_nats(self.app_state().nats_client.clone())
            })
    }

    pub async fn execute_tool_immediately(
        &self,
        tool: &AiTool,
        execution_params: Value,
        filesystem: &AgentFilesystem,
        shell: &ShellExecutor,
        context_title: &str,
    ) -> Result<Value, AppError> {
        let pipeline: Vec<String> = execution_params
            .get("pipeline")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let result = match &tool.configuration {
            AiToolConfiguration::Api(config) => {
                let result = self
                    .execute_api_tool(tool, config, &execution_params)
                    .await?;
                serde_json::to_value(result)?
            }
            AiToolConfiguration::PlatformEvent(config) => {
                let result = self
                    .execute_platform_event_tool(tool, config, &execution_params)
                    .await?;
                serde_json::to_value(result)?
            }
            AiToolConfiguration::PlatformFunction(config) => {
                let result = self
                    .execute_platform_function_tool(tool, config, &execution_params)
                    .await?;
                serde_json::to_value(result)?
            }
            AiToolConfiguration::Internal(config) => {
                self.execute_internal_tool(tool, config, &execution_params, filesystem, shell)
                    .await?
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

        let mut result = result;
        if result.is_object() && result.get("structure_hint").is_none() {
            let mut special_hint = None;
            for key in ["data", "result", "stdout"] {
                if let Some(val) = result.get(key) {
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

            let hint = special_hint.unwrap_or_else(|| result_shape::infer_schema_hint(&result));

            if let Some(obj) = result.as_object_mut() {
                obj.insert("structure_hint".to_string(), serde_json::json!(hint));
            }
        }

        let final_result = if !pipeline.is_empty() {
            let result_str = serde_json::to_string_pretty(&result)?;
            let transformed = shell.apply_pipeline(&result_str, &pipeline).await?;
            serde_json::json!({
                "result": transformed,
                "pipeline_applied": pipeline
            })
        } else {
            result
        };

        let mut final_result = final_result;
        let result_str = serde_json::to_string_pretty(&final_result)?;
        let char_count = result_str.chars().count();
        let threshold = 60_000;

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
        let _ = filesystem
            .write_file(&scratch_path, &result_str, None, None)
            .await;

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
                    "saved_to_path": scratch_path
                },
                "hint": format!(
                    "Output is larger than {} chars, so inline data is omitted. Read '{}' now (execution-scoped temp file) and filter with read_file/execute_command.",
                    threshold, scratch_path
                ),
            }));
        }

        if let Some(obj) = final_result.as_object_mut() {
            obj.insert(
                "saved_output_path".to_string(),
                serde_json::json!(scratch_path),
            );
            obj.insert(
                "output_notice".to_string(),
                serde_json::json!(
                    "Output is shown inline once and saved as an execution-scoped temp file."
                ),
            );
        } else {
            final_result = serde_json::json!({
                "result": final_result,
                "saved_output_path": scratch_path,
                "output_notice": "Output is shown inline once and saved as an execution-scoped temp file."
            });
        }

        Ok(final_result)
    }
}
