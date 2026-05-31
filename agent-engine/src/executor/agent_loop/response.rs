use super::core::AgentExecutor;

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use models::AgentThreadStatus;
use serde_json::{json, Value};

impl AgentExecutor {
    pub(super) fn sanitize_user_facing_message(raw: &str, fallback: &str) -> String {
        let stripped = Self::strip_leading_time_prefix(raw);
        let cleaned = stripped.trim();
        if cleaned.is_empty() {
            return fallback.to_string();
        }

        if Self::looks_like_internal_reasoning_dump(cleaned)
            || Self::looks_like_hallucinated_tool_render(cleaned)
        {
            return fallback.to_string();
        }

        cleaned.to_string()
    }

    fn strip_leading_time_prefix(text: &str) -> &str {
        let mut current = text.trim_start();
        for _ in 0..3 {
            if !current.starts_with('[') {
                return current;
            }
            let close = match current[1..].find(']') {
                Some(idx) => idx + 1,
                None => return current,
            };
            if close > 60 {
                return current;
            }
            let inner = current[1..close].trim();
            if !Self::looks_like_time_token(inner) {
                return current;
            }
            current = current[close + 1..].trim_start();
        }
        current
    }

    fn looks_like_time_token(s: &str) -> bool {
        if s == "just now" {
            return true;
        }
        if let Some(rest) = s.strip_prefix("at ") {
            return Self::looks_like_absolute_time_token(rest);
        }
        if let Some(rest) = s.strip_prefix("in ") {
            return Self::is_time_unit_token(rest);
        }
        if let Some(rest) = s.strip_suffix(" ago") {
            return Self::is_time_unit_token(rest);
        }
        Self::looks_like_absolute_time_token(s)
    }

    fn looks_like_absolute_time_token(s: &str) -> bool {
        let bytes = s.as_bytes();
        bytes.len() >= 10
            && bytes[0..4].iter().all(|b| b.is_ascii_digit())
            && bytes[4] == b'-'
            && bytes[5..7].iter().all(|b| b.is_ascii_digit())
            && bytes[7] == b'-'
    }

    fn is_time_unit_token(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let bytes = s.as_bytes();
        let last = bytes[bytes.len() - 1];
        if !matches!(last, b's' | b'm' | b'h' | b'd') {
            return false;
        }
        let digits = &bytes[..bytes.len() - 1];
        !digits.is_empty() && digits.iter().all(|b| b.is_ascii_digit())
    }

    pub(super) fn looks_like_hallucinated_tool_render(text: &str) -> bool {
        let pseudo_call_markers = [
            "+ bash:",
            "+ read_file:",
            "+ write_file:",
            "+ edit_file:",
            "+ note:",
            "+ task_graph_add_node:",
            "+ task_graph_complete_node:",
            "+ load_memory:",
            "+ save_memory:",
            "+ search_knowledgebase:",
            "+ web_search:",
            "+ url_content:",
            "[note:",
            "[Note:",
            "Action: ",
            "Action Input:",
        ];
        let pseudo_count: usize = pseudo_call_markers
            .iter()
            .map(|m| text.matches(m).count())
            .sum();
        let separator_lines = text.lines().filter(|l| l.trim() == "---").count();
        if pseudo_count >= 2 && separator_lines >= 2 {
            return true;
        }
        let mut block_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for chunk in text.split("\n\n") {
            let trimmed = chunk.trim();
            if trimmed.len() >= 60 {
                *block_counts.entry(trimmed).or_insert(0) += 1;
            }
        }
        block_counts.values().any(|c| *c >= 3)
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

        let data = result.cloned().unwrap_or(serde_json::Value::Null);
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
        for tool in self.virtual_tool_cache.values() {
            if loaded_tool_ids.contains(&tool.id) && self.tool_allowed_in_current_mode(&tool.name) {
                loaded_external_tools.push(tool.clone());
            }
        }
        loaded_external_tools
            .sort_by_key(|tool| load_order.get(&tool.id).copied().unwrap_or(usize::MAX));
        tools.extend(loaded_external_tools);

        tools
            .into_iter()
            .map(|mut tool| {
                let action = crate::tools::approval::resolve_approval_action(
                    &self.ctx.agent,
                    &tool.name,
                );
                let suffix = match action {
                    models::ApprovalAction::Allow => None,
                    models::ApprovalAction::Deny => Some(
                        " Denied by agent policy — calling this tool will return an error without executing.".to_string(),
                    ),
                    models::ApprovalAction::Review => {
                        let is_approved = effective_approved_tool_ids
                            .as_ref()
                            .map(|ids| ids.contains(&tool.id))
                            .unwrap_or_else(|| self.tool_is_approved(tool.id));
                        Some(if is_approved {
                            " Requires user approval, and approval is already active for this context.".to_string()
                        } else {
                            " Requires user approval. Runtime will request approval automatically if action execution selects it without an active grant.".to_string()
                        })
                    }
                };
                if let Some(suffix) = suffix {
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
