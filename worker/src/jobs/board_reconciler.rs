use anyhow::Result;
use chrono::Duration as ChronoDuration;
use commands::ReconcileStaleBoardItemsCommand;
use common::state::AppState;
use tracing::info;

pub const STALE_BOARD_ITEM_AFTER_HOURS: i64 = 1;
pub const MAX_BOARD_ITEMS_PER_TICK: i64 = 50;

pub const LEASE_KEY: &str = "wacht:jobs:board_reconciler:lease";
pub const LEASE_TTL_SECONDS: u64 = 900; // 15 minutes

pub async fn acquire_lease(app_state: &AppState, owner: &str) -> Result<bool> {
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await?;
    let acquired: Option<String> = redis::cmd("SET")
        .arg(LEASE_KEY)
        .arg(owner)
        .arg("NX")
        .arg("EX")
        .arg(LEASE_TTL_SECONDS)
        .query_async(&mut conn)
        .await?;
    Ok(acquired.is_some())
}

pub async fn release_lease(app_state: &AppState, owner: &str) -> Result<()> {
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await?;
    // Compare-and-delete atomically so we never drop a lease re-acquired by
    // another worker after TTL expiry.
    let script = redis::Script::new(
        r#"if redis.call("GET", KEYS[1]) == ARGV[1] then return redis.call("DEL", KEYS[1]) else return 0 end"#,
    );
    let _: i64 = script
        .key(LEASE_KEY)
        .arg(owner)
        .invoke_async(&mut conn)
        .await?;
    Ok(())
}

pub async fn reconcile_stale_board_items(app_state: &AppState) -> Result<String> {
    let command = ReconcileStaleBoardItemsCommand::new(
        ChronoDuration::hours(STALE_BOARD_ITEM_AFTER_HOURS),
        MAX_BOARD_ITEMS_PER_TICK,
    );

    let deps = common::deps::from_app(app_state).db().nats().id();
    let summary = command.execute_with_deps(&deps).await?;

    info!(
        rerouted_to_assignment = summary.rerouted_to_assignment,
        rerouted_to_coordinator = summary.rerouted_to_coordinator,
        skipped = summary.skipped,
        "Board reconciler tick completed"
    );

    Ok(format!(
        "Board reconciler: rerouted {} (assignment={}, coordinator={}), skipped {}",
        summary.total_rerouted(),
        summary.rerouted_to_assignment,
        summary.rerouted_to_coordinator,
        summary.skipped
    ))
}
