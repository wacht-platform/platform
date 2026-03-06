use common::error::AppError;
use common::state::AppState;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

use super::response;
use super::{GetChildStatusRequest, GetCompletionSummaryRequest};

const POLL_MIN_INTERVAL_SECONDS: u64 = 2;
const STALE_CHILD_SECONDS: i64 = 120;

#[derive(Serialize)]
struct ChildStatusItem {
    context_id: i64,
    title: String,
    status: String,
    latest_status_update: Option<String>,
    latest_status_at: Option<String>,
    completion_summary: Option<Value>,
}

#[derive(Serialize)]
struct ChildStatusData {
    children: Vec<ChildStatusItem>,
    count: usize,
    running_count: usize,
    recommended_sleep_ms: u64,
    max_running_seconds: i64,
    stale_children: Vec<i64>,
    timeout_reached: bool,
}

#[derive(Serialize)]
struct ChildCompletionSummaryData {
    child_context_id: i64,
    completion_summary: Option<Value>,
    message: String,
}

#[derive(Serialize)]
struct ChildrenCompletionSummaryData {
    children: Vec<queries::ChildCompletionSummary>,
    count: usize,
}

pub async fn get_child_status(
    app_state: &AppState,
    deployment_id: i64,
    parent_context_id: i64,
    tool_name: &str,
    request: GetChildStatusRequest,
) -> Result<Value, AppError> {
    enforce_poll_backoff(
        app_state,
        deployment_id,
        parent_context_id,
        "child_status",
        None,
    )
    .await?;

    let child_contexts_query = queries::GetChildContextsQuery {
        parent_context_id,
        deployment_id,
        include_completed: request.include_completed,
    };
    let children = child_contexts_query
        .execute_with(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await?;

    let child_context_ids: Vec<i64> = children.iter().map(|child| child.id).collect();
    let latest_updates_query =
        queries::GetLatestStatusUpdatesForContextsQuery::new(child_context_ids);
    let latest_updates = latest_updates_query
        .execute_with(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await?;
    let latest_update_by_context: HashMap<i64, queries::LatestStatusUpdate> = latest_updates
        .into_iter()
        .map(|update| (update.context_id, update))
        .collect();

    let mut running_count = 0usize;
    let mut max_running_seconds = 0i64;
    let mut stale_children = Vec::new();
    let now = chrono::Utc::now();
    for child in &children {
        if child.status.to_string() == "running" {
            running_count += 1;
            let last_progress_at = latest_update_by_context
                .get(&child.id)
                .map(|u| u.created_at)
                .unwrap_or(child.last_activity_at);
            let inactive_secs = (now - last_progress_at).num_seconds().max(0);
            max_running_seconds = max_running_seconds.max(inactive_secs);
            if inactive_secs >= STALE_CHILD_SECONDS {
                stale_children.push(child.id);
            }
        }
    }

    let children_with_status: Vec<ChildStatusItem> = children
        .into_iter()
        .map(|child| {
            let latest_update = latest_update_by_context.get(&child.id);
            ChildStatusItem {
                context_id: child.id,
                title: child.title,
                status: child.status.to_string(),
                latest_status_update: latest_update.map(|u| u.status_update.clone()),
                latest_status_at: latest_update.map(|u| u.created_at.to_rfc3339()),
                completion_summary: child.completion_summary,
            }
        })
        .collect();

    let count = children_with_status.len();
    response::success(
        tool_name,
        ChildStatusData {
            children: children_with_status,
            count,
            running_count,
            recommended_sleep_ms: if running_count > 0 { 2000 } else { 0 },
            max_running_seconds,
            timeout_reached: !stale_children.is_empty(),
            stale_children,
        },
    )
}

pub async fn completion_summary(
    app_state: &AppState,
    deployment_id: i64,
    parent_context_id: i64,
    tool_name: &str,
    request: GetCompletionSummaryRequest,
) -> Result<Value, AppError> {
    if let Some(child_context_id) = request.child_context_id {
        enforce_poll_backoff(
            app_state,
            deployment_id,
            parent_context_id,
            "completion_summary",
            Some(child_context_id.0),
        )
        .await?;
        let summary_query =
            queries::GetChildCompletionSummaryQuery::new(child_context_id.0, deployment_id)
                .with_parent_context(parent_context_id);
        let summary = summary_query
            .execute_with(
                app_state
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await?;

        return response::success(
            tool_name,
            ChildCompletionSummaryData {
                child_context_id: child_context_id.0,
                completion_summary: summary.clone(),
                message: if summary.is_some() {
                    "Child has completed".to_string()
                } else {
                    "Child has not yet completed".to_string()
                },
            },
        );
    }

    enforce_poll_backoff(
        app_state,
        deployment_id,
        parent_context_id,
        "completion_summary",
        None,
    )
    .await?;

    let summaries_query =
        queries::GetChildrenCompletionSummariesQuery::new(parent_context_id, deployment_id);
    let summaries = summaries_query
        .execute_with(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await?;

    let count = summaries.len();
    response::success(
        tool_name,
        ChildrenCompletionSummaryData {
            children: summaries,
            count,
        },
    )
}

async fn enforce_poll_backoff(
    app_state: &AppState,
    deployment_id: i64,
    parent_context_id: i64,
    kind: &str,
    child_context_id: Option<i64>,
) -> Result<(), AppError> {
    let scope = child_context_id
        .map(|id| format!("child:{id}"))
        .unwrap_or_else(|| "all".to_string());
    let key = format!(
        "agent_swarm:poll_backoff:{}:{}:{}:{}",
        deployment_id, parent_context_id, kind, scope
    );

    let too_soon =
        super::guards::acquire_dedupe_token(app_state, &key, POLL_MIN_INTERVAL_SECONDS).await?;
    if too_soon {
        return Err(AppError::BadRequest(format!(
            "Polling too frequently for {}. Wait at least {} seconds (use sleep tool) before polling again.",
            kind, POLL_MIN_INTERVAL_SECONDS
        )));
    }

    Ok(())
}
