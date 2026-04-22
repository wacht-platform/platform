use super::core::AgentExecutor;

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use models::AgentThreadStatus;
use serde_json::{json, Value};

impl AgentExecutor {
    pub(super) fn sanitize_user_facing_message(raw: &str, fallback: &str) -> String {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            return fallback.to_string();
        }

        if Self::looks_like_internal_reasoning_dump(cleaned) {
            return fallback.to_string();
        }

        cleaned.to_string()
    }

    pub(super) fn looks_like_internal_reasoning_dump(text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        let markers = [
            "the user is asking",
            "i need to perform",
            "universal search across all categories",
            "user requested cancellation",
            "internal reasoning",
        ];
        let marker_hits = markers.iter().filter(|m| lower.contains(**m)).count();

        let numbered_lines = text
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
                digits > 0 && trimmed.chars().nth(digits) == Some('.')
            })
            .count();

        marker_hits >= 2 || (marker_hits >= 1 && numbered_lines >= 3)
    }

    pub(crate) fn standardize_tool_output(
        &self,
        tool_name: &str,
        result: Option<&Value>,
        error_message: Option<String>,
    ) -> Value {
        let status = if error_message.is_some() {
            "error"
        } else if result
            .and_then(|r| r.get("status"))
            .and_then(|s| s.as_str())
            == Some("pending")
        {
            "pending"
        } else {
            "success"
        };

        let mut data = result.cloned().unwrap_or(serde_json::Value::Null);
        let structure_hint = data
            .get("structure_hint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(obj) = data.as_object_mut() {
            obj.remove("structure_hint");
        }
        let truncated = result
            .and_then(|r| r.get("truncated"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let size_bytes = result
            .and_then(|r| r.get("original_stats"))
            .and_then(|s| s.get("size_bytes"))
            .and_then(|v| v.as_u64());
        let saved_output_path = result
            .and_then(|r| r.get("saved_output_path"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                result
                    .and_then(|r| r.get("original_stats"))
                    .and_then(|s| s.get("saved_to_path"))
                    .and_then(|v| v.as_str())
            });

        json!({
            "schema_version": 1,
            "tool_name": tool_name,
            "status": status,
            "error": error_message.map(|msg| json!({
                "code": "tool_execution_error",
                "message": msg,
            })),
            "data": data,
            "meta": {
                "truncated": truncated,
                "structure_hint": structure_hint,
                "size_bytes": size_bytes,
                "saved_output_path": saved_output_path,
                "generated_at": chrono::Utc::now().to_rfc3339(),
            }
        })
    }

    pub(crate) async fn finish_without_summary(&mut self) -> Result<(), AppError> {
        self.apply_thread_status(
            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
                .with_execution_state(self.build_execution_state_snapshot(None)),
            AgentThreadStatus::Idle,
        )
        .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
        .await?;

        Ok(())
    }

    pub(crate) async fn available_tools_for_mode(&self) -> Vec<models::AiTool> {
        let effective_approved_tool_ids = self.effective_approved_tool_ids().await.ok();
        let mut tools: Vec<models::AiTool> = self
            .ctx
            .agent
            .tools
            .iter()
            .filter(|t| matches!(t.tool_type, models::AiToolType::Internal))
            .filter(|t| self.tool_allowed_in_current_mode(&t.name))
            .cloned()
            .collect();

        let loaded_tool_ids = self
            .loaded_external_tool_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        let load_order = self
            .loaded_external_tool_ids
            .iter()
            .enumerate()
            .map(|(idx, id)| (*id, idx))
            .collect::<std::collections::HashMap<_, _>>();
        let mut loaded_external_tools = self
            .ctx
            .agent
            .tools
            .iter()
            .filter(|t| !matches!(t.tool_type, models::AiToolType::Internal))
            .filter(|t| loaded_tool_ids.contains(&t.id))
            .filter(|t| self.tool_allowed_in_current_mode(&t.name))
            .cloned()
            .collect::<Vec<_>>();
        loaded_external_tools
            .sort_by_key(|tool| load_order.get(&tool.id).copied().unwrap_or(usize::MAX));
        tools.extend(loaded_external_tools);

        tools
            .into_iter()
            .map(|mut tool| {
                if tool.requires_user_approval {
                    let is_approved = effective_approved_tool_ids
                        .as_ref()
                        .map(|ids| ids.contains(&tool.id))
                        .unwrap_or_else(|| self.tool_is_approved(tool.id));
                    let suffix = if is_approved {
                        " Requires user approval, and approval is already active for this context."
                    } else {
                        " Requires user approval. Runtime will request approval automatically if action execution selects it without an active grant."
                    };
                    let description = tool.description.unwrap_or_default();
                    tool.description = Some(format!("{description}{suffix}").trim().to_string());
                }
                tool
            })
            .collect()
    }

    fn tool_is_approved(&self, tool_id: i64) -> bool {
        self.approved_always_tool_ids.contains(&tool_id)
    }
}
