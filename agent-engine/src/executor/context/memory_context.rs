use crate::runtime::thread_execution_context::ThreadExecutionContext;
use common::state::AppState;
use common::{error::AppError, get_startup_memories_in_table};
use models::{ImmediateContext, MemoryRecord};
use queries::GetLLMConversationHistoryQuery;
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
    let json: String = conn
        .get(startup_memory_cache_key(thread_id))
        .await
        .ok()?;
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
    let (mru_memories, recent_conversations) = tokio::join!(
        load_startup_memories(ctx, 50),
        load_recent_conversations(ctx, board_item_id),
    );
    Ok(ImmediateContext {
        memories: mru_memories?,
        conversations: recent_conversations?,
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
    let Some(table) = ctx.get_memory_table().await? else {
        write_cached_startup_memories(&ctx.app_state, ctx.thread_id, &[]).await;
        return Ok(Vec::new());
    };
    let memories = get_startup_memories_in_table(
        &table,
        ctx.agent.deployment_id,
        ctx.thread_id,
        thread.actor_id,
        limit,
        ctx.provider_keys.embedding_dimension,
    )
    .await?;
    write_cached_startup_memories(&ctx.app_state, ctx.thread_id, &memories).await;
    Ok(memories)
}

async fn load_recent_conversations(
    ctx: &Arc<ThreadExecutionContext>,
    board_item_id: Option<i64>,
) -> Result<Vec<models::ConversationRecord>, AppError> {
    GetLLMConversationHistoryQuery::new(ctx.thread_id)
        .with_board_item_id(board_item_id)
        .execute_with_db(ctx.app_state.db_router.writer())
        .await
}

