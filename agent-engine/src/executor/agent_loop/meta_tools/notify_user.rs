use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use models::{ConversationContent, ConversationMessageType};

use crate::executor::core::AgentExecutor;

impl AgentExecutor {
    pub(in crate::executor::agent_loop) async fn handle_notify_user_call(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
    ) -> Result<bool, AppError> {
        let args: dto::json::agent_executor::NotifyUserParams =
            serde_json::from_value(call.arguments.clone())
                .map_err(|e| AppError::BadRequest(format!("notify_user params malformed: {e}")))?;
        let message = args.message.trim();
        if message.is_empty() {
            return Err(AppError::BadRequest(
                "notify_user requires a non-empty message".to_string(),
            ));
        }
        let safe_message = Self::sanitize_user_facing_message(message, "Posted a status update.");
        self.store_conversation(
            ConversationContent::Steer {
                message: safe_message,
                further_actions_required: false,
                reasoning: "Terminal text response — no further tool calls emitted.".to_string(),
                attachments: None,
            },
            ConversationMessageType::Steer,
        )
        .await?;

        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(self.build_execution_state_snapshot(None))
            .with_status(models::AgentThreadStatus::Idle)
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        Ok(false)
    }
}
