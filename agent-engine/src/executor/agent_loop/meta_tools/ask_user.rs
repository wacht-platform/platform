use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use models::{ConversationContent, ConversationMessageType};

use crate::executor::core::AgentExecutor;

impl AgentExecutor {
    pub(in crate::executor::agent_loop) async fn handle_ask_user_call(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
    ) -> Result<bool, AppError> {
        use dto::json::ask_user::{validate_question_set, AskUserParams};

        let params: AskUserParams = serde_json::from_value(call.arguments.clone())
            .map_err(|e| AppError::BadRequest(format!("ask_user params malformed: {e}")))?;

        if let Err(e) = validate_question_set(&params.questions) {
            return Err(AppError::BadRequest(format!("ask_user invalid: {e}")));
        }

        self.invalidate_stale_pending_question();
        if self.pending_question.is_some() || self.board_item_has_pending_question().await? {
            self.store_transient_steer(
                "ask_user_blocked_by_pending_question",
                "ask_user blocked: active pending question already on this thread/task. Wait for user answer before asking new.".to_string(),
            );
            return Ok(true);
        }

        let assignment_id = self
            .active_thread_event
            .as_ref()
            .and_then(|event| event.assignment_execution_payload())
            .map(|payload| payload.assignment_id);
        let pending = models::PendingQuestion {
            questions: params.questions.clone(),
            context: params.context.clone(),
            asked_at: chrono::Utc::now(),
            asked_by_thread_id: self.ctx.thread_id,
            asked_by_assignment_id: assignment_id,
        };

        let questions_value = serde_json::to_value(&params.questions).map_err(|e| {
            AppError::Internal(format!("ask_user: failed to serialize questions: {e}"))
        })?;
        self.store_conversation(
            ConversationContent::ClarificationRequest {
                questions: questions_value,
                context: params.context.clone(),
            },
            ConversationMessageType::ClarificationRequest,
        )
        .await?;

        if let Some(board_item_id) = self.current_board_item_id() {
            if self.board_item_has_pending_question().await? {
                self.store_transient_steer(
                    "ask_user_blocked_by_pending_question",
                    "ask_user blocked: a concurrent execution already set a pending question on this board item.".to_string(),
                );
                return Ok(true);
            }
            commands::SetBoardItemPendingQuestionCommand {
                board_item_id,
                pending_question: Some(pending.clone()),
            }
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        }

        let pre_set_pending = self.pending_question.replace(pending);
        let execution_state = self.build_execution_state_snapshot(None);
        let thread_update = self
            .apply_thread_status(
                UpdateAgentThreadStateCommand::new(
                    self.ctx.thread_id,
                    self.ctx.agent.deployment_id,
                )
                .with_execution_state(execution_state),
                models::AgentThreadStatus::WaitingForInput,
            )
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await;

        if let Err(e) = thread_update {
            self.pending_question = pre_set_pending;
            return Err(e);
        }

        Ok(false)
    }

    fn invalidate_stale_pending_question(&mut self) {
        let Some(pending) = self.pending_question.as_ref() else {
            return;
        };
        let active_assignment_id = self
            .active_thread_event
            .as_ref()
            .and_then(|event| event.assignment_execution_payload())
            .map(|payload| payload.assignment_id);
        if pending.asked_by_assignment_id != active_assignment_id {
            self.pending_question = None;
        }
    }

    async fn board_item_has_pending_question(&self) -> Result<bool, AppError> {
        let Some(board_item_id) = self.current_board_item_id() else {
            return Ok(false);
        };
        let item = queries::GetProjectTaskBoardItemByIdQuery::new(board_item_id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?;
        Ok(item.and_then(|i| i.pending_question).is_some())
    }
}
