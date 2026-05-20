use crate::runtime::thread_execution_context::ThreadExecutionContext;
use common::error::AppError;
use common::state::AppState;
use models::{ImmediateContext, MemoryRecord, TaskRoutingEvent, TaskThreadMeta};
use queries::{
    GetBoardItemConversationHistoryQuery, GetBoardItemRoutingEventsQuery,
    GetBoardItemThreadMetaQuery, GetLLMConversationHistoryQuery,
};
use redis::AsyncCommands;
use std::sync::Arc;

const STARTUP_MEMORY_CACHE_TTL_SECONDS: u64 = 600;

fn startup_memory_cache_key(thread_id: i64) -> String {
    format!("agent:startup_memories:{thread_id}")
}

async fn read_cached_startup_memories(
    app_state: &AppState,
    thread_id: i64,
) -> Option<Vec<MemoryRecord>> {
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .ok()?;
    let json: String = conn.get(startup_memory_cache_key(thread_id)).await.ok()?;
    serde_json::from_str(&json).ok()
}

async fn write_cached_startup_memories(
    app_state: &AppState,
    thread_id: i64,
    memories: &[MemoryRecord],
) {
    let Ok(json) = serde_json::to_string(memories) else {
        return;
    };
    let Ok(mut conn) = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
    else {
        return;
    };
    let _: Result<(), _> = conn
        .set_ex(
            startup_memory_cache_key(thread_id),
            json,
            STARTUP_MEMORY_CACHE_TTL_SECONDS,
        )
        .await;
}

pub(crate) async fn invalidate_startup_memory_cache(app_state: &AppState, thread_id: i64) {
    let Ok(mut conn) = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
    else {
        return;
    };
    let _: Result<(), _> = conn.del(startup_memory_cache_key(thread_id)).await;
}

pub(crate) async fn load_immediate_context(
    ctx: &Arc<ThreadExecutionContext>,
    board_item_id: Option<i64>,
) -> Result<ImmediateContext, AppError> {
    let (mru_memories, recent_conversations, routing_events, thread_meta) = tokio::join!(
        load_startup_memories(ctx, 50),
        load_recent_conversations(ctx, board_item_id),
        load_routing_events(ctx, board_item_id),
        load_task_thread_meta(ctx, board_item_id),
    );
    Ok(ImmediateContext {
        memories: mru_memories?,
        conversations: recent_conversations?,
        routing_events: routing_events?,
        task_thread_meta: thread_meta?,
    })
}

async fn load_startup_memories(
    ctx: &Arc<ThreadExecutionContext>,
    limit: usize,
) -> Result<Vec<MemoryRecord>, AppError> {
    if let Some(cached) = read_cached_startup_memories(&ctx.app_state, ctx.thread_id).await {
        return Ok(cached);
    }
    let thread = ctx.get_thread().await?;
    let memories = ctx
        .vector_store
        .get_startup_memories(ctx.thread_id, thread.actor_id, limit)
        .await?;
    write_cached_startup_memories(&ctx.app_state, ctx.thread_id, &memories).await;
    Ok(memories)
}

async fn load_recent_conversations(
    ctx: &Arc<ThreadExecutionContext>,
    board_item_id: Option<i64>,
) -> Result<Vec<models::ConversationRecord>, AppError> {
    if let Some(board_item_id) = board_item_id {
        GetBoardItemConversationHistoryQuery::new(board_item_id, ctx.thread_id)
            .execute_with_db(ctx.app_state.db_router.writer())
            .await
    } else {
        GetLLMConversationHistoryQuery::new(ctx.thread_id)
            .execute_with_db(ctx.app_state.db_router.writer())
            .await
    }
}

async fn load_task_thread_meta(
    ctx: &Arc<ThreadExecutionContext>,
    board_item_id: Option<i64>,
) -> Result<Vec<TaskThreadMeta>, AppError> {
    let Some(board_item_id) = board_item_id else {
        return Ok(Vec::new());
    };
    let rows = GetBoardItemThreadMetaQuery::new(board_item_id)
        .execute_with_db(ctx.app_state.db_router.writer())
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| TaskThreadMeta {
            thread_id: r.thread_id,
            title: r.title,
            thread_purpose: r.thread_purpose,
        })
        .collect())
}

async fn load_routing_events(
    ctx: &Arc<ThreadExecutionContext>,
    board_item_id: Option<i64>,
) -> Result<Vec<TaskRoutingEvent>, AppError> {
    let Some(board_item_id) = board_item_id else {
        return Ok(Vec::new());
    };
    let rows = GetBoardItemRoutingEventsQuery::new(board_item_id)
        .execute_with_db(ctx.app_state.db_router.writer())
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| TaskRoutingEvent {
            id: r.id,
            coordinator_thread_id: r.coordinator_thread_id,
            routing_reason: r.routing_reason,
            summary: r.summary,
            note: r.note,
            created_at: r.created_at,
        })
        .collect())
}
