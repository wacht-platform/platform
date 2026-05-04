use commands::CleanupCompactedConversationsCommand;
use common::state::AppState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct CompactedConversationCleanupTask {
    pub thread_id: i64,
    pub cleanup_through_id: i64,
    #[serde(default)]
    pub board_item_id: Option<i64>,
}

pub async fn cleanup_compacted_conversations(
    task: CompactedConversationCleanupTask,
    app_state: &AppState,
) -> Result<String, String> {
    let deleted_count =
        CleanupCompactedConversationsCommand::new(task.thread_id, task.cleanup_through_id)
            .with_board_item_id(task.board_item_id)
            .execute_with_db(app_state.db_router.writer())
            .await
            .map_err(|e| e.to_string())?;

    Ok(format!(
        "Conversation cleanup completed: thread_id={} cleanup_through_id={} board_item_id={:?} deleted_count={}",
        task.thread_id, task.cleanup_through_id, task.board_item_id, deleted_count
    ))
}
