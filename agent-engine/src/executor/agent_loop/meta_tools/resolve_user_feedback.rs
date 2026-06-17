use common::error::AppError;

use crate::executor::core::AgentExecutor;

impl AgentExecutor {
    pub(in crate::executor::agent_loop) async fn handle_resolve_user_feedback_call(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
    ) -> Result<u64, AppError> {
        let args: dto::json::agent_executor::ResolveUserFeedbackParams =
            serde_json::from_value(call.arguments.clone()).map_err(|e| {
                AppError::BadRequest(format!("resolve_user_feedback params malformed: {e}"))
            })?;
        let resolution = args.resolution.trim().to_string();
        if resolution.is_empty() {
            return Err(AppError::BadRequest(
                "resolve_user_feedback requires a non-empty resolution summary".to_string(),
            ));
        }
        let Some(board_item_id) = self.current_board_item_id() else {
            return Err(AppError::BadRequest(
                "resolve_user_feedback can only be called when a board item is active".to_string(),
            ));
        };
        let (comment_ids, invalid_ids): (Vec<i64>, Vec<&String>) =
            args.comment_ids
                .iter()
                .fold((Vec::new(), Vec::new()), |(mut ok, mut bad), raw| {
                    match raw.parse::<i64>() {
                        Ok(id) => ok.push(id),
                        Err(_) => bad.push(raw),
                    }
                    (ok, bad)
                });
        if !invalid_ids.is_empty() {
            return Err(AppError::BadRequest(format!(
                "resolve_user_feedback: invalid comment_ids: {:?}",
                invalid_ids
            )));
        }
        if comment_ids.is_empty() {
            return Err(AppError::BadRequest(
                "resolve_user_feedback requires at least one valid comment_id".to_string(),
            ));
        }
        let rows_affected = commands::ResolveBoardItemCommentsCommand {
            board_item_id,
            comment_ids: comment_ids.clone(),
            resolved_by_thread_id: self.ctx.thread_id,
            resolution_summary: resolution,
        }
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;
        tracing::info!(
            target: "loop",
            board_item_id,
            thread_id = self.ctx.thread_id,
            comment_ids = ?comment_ids,
            rows_affected,
            "resolve_user_feedback"
        );
        Ok(rows_affected)
    }
}
