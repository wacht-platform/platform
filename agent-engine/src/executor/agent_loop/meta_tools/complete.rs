use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use models::{AgentThreadStatus, ConversationContent, ConversationMessageType};
use serde_json::Value;

use crate::executor::core::AgentExecutor;

#[derive(Debug, Clone)]
pub(in crate::executor) struct CompletionHandoff {
    pub summary: String,
    pub artifacts: Option<Value>,
    pub blockers: Option<Value>,
    pub next_actions: Option<Value>,
}

impl CompletionHandoff {
    pub fn from_summary(summary: String) -> Self {
        Self {
            summary,
            artifacts: None,
            blockers: None,
            next_actions: None,
        }
    }
}

impl AgentExecutor {
    pub(in crate::executor::agent_loop) async fn handle_complete_call(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
        accompanying_text: Option<&str>,
    ) -> Result<bool, AppError> {
        let summary = call
            .arguments
            .get("summary")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(summary) = summary else {
            self.record_invalid_tool_call(
                "complete",
                &call.arguments,
                "`complete` requires a non-empty `summary` (1-3 sentences: what was accomplished, key decisions, resulting state).",
            )
            .await?;
            return Ok(true);
        };

        if let Some(error) = self.completion_guard_error().await? {
            self.record_invalid_tool_call("complete", &call.arguments, &error)
                .await?;
            return Ok(true);
        }

        let handoff = CompletionHandoff {
            summary: summary.to_string(),
            artifacts: call
                .arguments
                .get("artifacts")
                .cloned()
                .filter(|v| !v.is_null()),
            blockers: call
                .arguments
                .get("blockers")
                .cloned()
                .filter(|v| !v.is_null()),
            next_actions: call
                .arguments
                .get("next_actions")
                .cloned()
                .filter(|v| !v.is_null()),
        };

        let final_message = accompanying_text
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| Self::sanitize_user_facing_message(t, summary))
            .unwrap_or_else(|| summary.to_string());

        self.finalize_completion(handoff, final_message).await
    }

    /// Completion preconditions shared by `complete` and the text-only
    /// auto-complete fallback. `Some(error)` means completion must be refused.
    pub(in crate::executor::agent_loop) async fn completion_guard_error(
        &mut self,
    ) -> Result<Option<String>, AppError> {
        if self.is_service_mode_execution() && !self.service_mode_journal_was_updated().await? {
            return Ok(Some(
                "Completion rejected: `/task/JOURNAL.md` is still empty this run. Append a short concrete entry (did/found/left) with `append_file`, then call `complete`.".to_string(),
            ));
        }

        // Feedback resolution is coordinator-owned. Only the coordinator is
        // blocked on unresolved task comments; executors/reviewers never own them.
        let unresolved_ids = if self.is_coordinator_thread {
            self.unresolved_feedback_ids().await?
        } else {
            Vec::new()
        };
        if !unresolved_ids.is_empty() {
            let ids_csv = unresolved_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(Some(format!(
                "Completion rejected: feedback comment(s) {ids_csv} are still [unresolved]. Close each via `resolve_user_feedback` (one call per id, with a one-line summary) — if you already acted on it, call the tool now; if no action is needed, call it with the explanation — then call `complete`."
            )));
        }

        if let Some(block) = self.completion_block().await? {
            return Ok(Some(block.tool_error()));
        }

        Ok(None)
    }

    pub(in crate::executor::agent_loop) async fn finalize_completion(
        &mut self,
        handoff: CompletionHandoff,
        final_message: String,
    ) -> Result<bool, AppError> {
        self.complete_nudge_count = 0;

        self.persist_task_handoff_summary(&handoff).await;

        self.store_conversation(
            ConversationContent::Steer {
                message: final_message,
                further_actions_required: false,
                reasoning: "Run completed — terminal handoff recorded.".to_string(),
                attachments: None,
            },
            ConversationMessageType::Steer,
        )
        .await?;

        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(self.build_execution_state_snapshot(None))
            .with_status(AgentThreadStatus::Idle)
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        Ok(false)
    }
}
