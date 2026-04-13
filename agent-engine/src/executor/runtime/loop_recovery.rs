use super::core::AgentExecutor;
use super::step_control::{
    classify_runtime_correction, RuntimeCorrectionContext, RuntimeCorrectionKind,
};
use crate::executor::tools::tool_params::ToolExecutionLoopOutcome;

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use models::{AgentThreadStatus, ConversationContent, ConversationMessageType};
use tokio::time::{sleep, Duration};

const MAX_CONSECUTIVE_RECOVERY_ATTEMPTS: usize = 8;
const DATABASE_ERROR_RETRY_BASE_MS: u64 = 2_000;
const DATABASE_ERROR_RETRY_MAX_MS: u64 = 120_000;
const EXECUTION_ERROR_RETRY_BASE_MS: u64 = 2_000;
const EXECUTION_ERROR_RETRY_MAX_MS: u64 = 120_000;

impl AgentExecutor {
    pub(crate) async fn handle_loop_error(
        &mut self,
        stage: &str,
        error: AppError,
        consecutive_errors: &mut usize,
    ) -> Result<(), AppError> {
        let error_text = error.to_string();
        let database_error = matches!(&error, AppError::Database(_));
        *consecutive_errors += 1;
        let should_emit_correction = *consecutive_errors == MAX_CONSECUTIVE_RECOVERY_ATTEMPTS;
        let correction_already_emitted = *consecutive_errors > MAX_CONSECUTIVE_RECOVERY_ATTEMPTS;

        if database_error {
            if correction_already_emitted {
                return Err(error);
            }

            if should_emit_correction {
                self.store_runtime_correction(RuntimeCorrectionKind::DatabaseErrorRetry)
                    .await?;
            } else {
                sleep(Self::calculate_backoff_delay(
                    *consecutive_errors,
                    DATABASE_ERROR_RETRY_BASE_MS,
                    DATABASE_ERROR_RETRY_MAX_MS,
                ))
                .await;
            }
            return Ok(());
        }

        if let Some(correction) = classify_runtime_correction(&RuntimeCorrectionContext {
            stage,
            error_text: &error_text,
            loaded_external_tool_ids: &self.loaded_external_tool_ids,
            agent_tools: &self.ctx.agent.tools,
        }) {
            if correction_already_emitted {
                return Err(error);
            }

            if should_emit_correction {
                self.store_runtime_correction(correction).await?;
            } else {
                sleep(Self::calculate_backoff_delay(
                    *consecutive_errors,
                    EXECUTION_ERROR_RETRY_BASE_MS,
                    EXECUTION_ERROR_RETRY_MAX_MS,
                ))
                .await;
            }
            return Ok(());
        }

        let correction = if Self::is_llm_stage(stage)
            && matches!(
                &error,
                AppError::Internal(_) | AppError::Timeout | AppError::External(_)
            ) {
            RuntimeCorrectionKind::LlmRequestFailed
        } else {
            RuntimeCorrectionKind::RetryableExecutionError
        };

        if correction_already_emitted {
            return Err(error);
        }

        if should_emit_correction {
            self.store_runtime_correction(correction).await?;
        } else {
            sleep(Self::calculate_backoff_delay(
                *consecutive_errors,
                EXECUTION_ERROR_RETRY_BASE_MS,
                EXECUTION_ERROR_RETRY_MAX_MS,
            ))
            .await;
        }

        Ok(())
    }

    fn calculate_backoff_delay(attempt: usize, base_ms: u64, max_ms: u64) -> Duration {
        let backoff_ms = (base_ms << ((attempt.saturating_sub(1)).min(7) as u32)).min(max_ms);
        Duration::from_millis(backoff_ms)
    }

    fn is_llm_stage(stage: &str) -> bool {
        matches!(stage, "next-step-decision" | "decision-processing")
    }

    pub(crate) async fn store_runtime_correction(
        &mut self,
        correction: RuntimeCorrectionKind,
    ) -> Result<(), AppError> {
        self.store_conversation(
            ConversationContent::SystemDecision {
                step: correction.step().to_string(),
                reasoning: correction.reasoning().to_string(),
                confidence: correction.confidence(),
            },
            ConversationMessageType::SystemDecision,
        )
        .await
    }

    pub(crate) fn selected_tool_call_log_fields(
        decision: &dto::json::agent_executor::NextStepDecision,
    ) -> (usize, Vec<String>) {
        match decision.next_step {
            dto::json::agent_executor::NextStep::SearchTools => {
                let labels = decision
                    .search_tools_directive
                    .as_ref()
                    .map(|directive| directive.queries.clone())
                    .unwrap_or_default();
                (labels.len(), labels)
            }
            dto::json::agent_executor::NextStep::LoadTools => {
                let labels = decision
                    .load_tools_directive
                    .as_ref()
                    .map(|directive| directive.tool_names.clone())
                    .unwrap_or_default();
                (labels.len(), labels)
            }
            _ => (0, Vec::new()),
        }
    }

    pub(crate) async fn finalize_action_execution_outcome(
        &mut self,
        outcome: ToolExecutionLoopOutcome,
    ) -> Result<bool, AppError> {
        let any_pending = outcome.any_pending;

        if any_pending {
            let thread_state = queries::GetAgentThreadStateQuery::new(
                self.ctx.thread_id,
                self.ctx.agent.deployment_id,
            )
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await?;
            let pending_approval_request = thread_state
                .execution_state
                .as_ref()
                .and_then(|state| state.pending_approval_request.clone());

            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
                .with_execution_state(self.build_execution_state_snapshot(pending_approval_request))
                .with_status(AgentThreadStatus::WaitingForInput)
                .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
                .await?;
        } else {
            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
                .with_execution_state(self.build_execution_state_snapshot(None))
                .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
                .await?;
        }

        self.invalidate_task_graph_snapshot();

        Ok(!any_pending)
    }
}
