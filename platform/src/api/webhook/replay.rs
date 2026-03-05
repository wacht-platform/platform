use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use common::state::AppState;
use dto::json::webhook_requests::{
    ReplayTaskCancelResponse, ReplayTaskListQuery, ReplayTaskListResponse,
    ReplayTaskStatusResponse, ReplayWebhookDeliveryRequest, ReplayWebhookDeliveryResponse,
};
use queries::{GetWebhookAppByNameQuery, Query as QueryTrait};
use redis::{AsyncCommands, Script};

use crate::api::pagination::paginate_results;
use crate::application::response::{ApiError, ApiErrorResponse, ApiResult};
use crate::middleware::RequireDeployment;

const LUA_REPLAY_RESERVE: &str = r#"
        local idem_key = KEYS[1]
        local active_key = KEYS[2]
        local pending = ARGV[1]
        local idem_ttl = tonumber(ARGV[2])
        local max_active = tonumber(ARGV[3])
        local active_ttl = tonumber(ARGV[4])
        local existing = redis.call('GET', idem_key)
        if existing then
          return {1, existing}
        end

        local current_active = tonumber(redis.call('GET', active_key) or '0')
        if current_active >= max_active then
          return {2, ''}
        end

        redis.call('SET', idem_key, pending, 'EX', idem_ttl, 'NX')
        local active_after = tonumber(redis.call('INCR', active_key))
        if active_after == 1 then
          redis.call('EXPIRE', active_key, active_ttl)
        end
        if active_after > max_active then
          redis.call('DECR', active_key)
          local idem_val = redis.call('GET', idem_key)
          if idem_val == pending then
            redis.call('DEL', idem_key)
          end
          return {2, ''}
        end
        return {0, ''}
        "#;

const LUA_REPLAY_FINALIZE: &str = r#"
        local key = KEYS[1]
        local expected = ARGV[1]
        local final_value = ARGV[2]
        local ttl = tonumber(ARGV[3])
        local existing = redis.call('GET', key)
        if not existing then
          return 0
        end
        if existing ~= expected then
          return -1
        end
        redis.call('SET', key, final_value, 'EX', ttl)
        return 1
        "#;

const LUA_REPLAY_CANCEL: &str = r#"
        local snapshot_key = KEYS[1]
        local active_key = KEYS[2]
        local now = ARGV[1]
        local ttl = tonumber(ARGV[2])

        redis.call('HSET', snapshot_key, 'status', 'cancelled')
        redis.call('HSET', snapshot_key, 'cancelled', '1')
        redis.call('HSET', snapshot_key, 'cancelled_at', now)
        redis.call('HSET', snapshot_key, 'completed_at', now)

        local reserved = redis.call('HGET', snapshot_key, 'active_slot_reserved')
        if reserved == '1' then
          redis.call('HSET', snapshot_key, 'active_slot_reserved', '0')
          local current_active = tonumber(redis.call('GET', active_key) or '0')
          if current_active > 0 then
            current_active = tonumber(redis.call('DECR', active_key))
          end
          if current_active <= 0 then
            redis.call('DEL', active_key)
          end
        end

        redis.call('EXPIRE', snapshot_key, ttl)
        return 1
        "#;

const LUA_REPLAY_ROLLBACK_SLOT: &str = r#"
        local idem_key = KEYS[1]
        local active_key = KEYS[2]
        local expected_pending = ARGV[1]

        local idem_value = redis.call('GET', idem_key)
        if idem_value == expected_pending then
          redis.call('DEL', idem_key)
        end

        local current_active = tonumber(redis.call('GET', active_key) or '0')
        if current_active > 0 then
          current_active = tonumber(redis.call('DECR', active_key))
        end
        if current_active <= 0 then
          redis.call('DEL', active_key)
        end
        return 1
        "#;

const ERR_CODE_REPLAY_MAX_IDS_EXCEEDED: &str = "REPLAY_MAX_IDS_EXCEEDED";
const ERR_CODE_REPLAY_DATE_WINDOW_EXCEEDED: &str = "REPLAY_DATE_WINDOW_EXCEEDED";
const ERR_CODE_REPLAY_CONCURRENCY_EXCEEDED: &str = "REPLAY_CONCURRENCY_EXCEEDED";

fn validate_replay_status(status: &str) -> bool {
    matches!(
        status,
        "success" | "failed" | "permanently_failed" | "filtered"
    )
}

fn replay_bad_request(_code: &str, message: impl Into<String>) -> ApiErrorResponse {
    let status = StatusCode::BAD_REQUEST;
    (
        status,
        ApiError {
            message: message.into(),
            code: status.as_u16(),
        },
    )
        .into()
}

pub async fn replay_webhook_delivery(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(request): Json<ReplayWebhookDeliveryRequest>,
) -> ApiResult<ReplayWebhookDeliveryResponse> {
    use dto::json::nats::{NatsTaskMessage, WebhookReplayBatchPayload};
    const MAX_IDS_PER_REPLAY: usize = 500;
    const MAX_REPLAY_WINDOW_HOURS: i64 = 48;
    const MAX_ACTIVE_REPLAY_TASKS: i32 = 3;
    const REPLAY_IDEMPOTENCY_TTL_SECS: i64 = 1800;
    const REPLAY_ACTIVE_COUNT_TTL_SECS: i64 = 86400;
    const RESERVE_RESULT_EXISTS: i32 = 1;
    const RESERVE_RESULT_LIMIT: i32 = 2;

    // Ensure app belongs to deployment
    GetWebhookAppByNameQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    let now = chrono::Utc::now();
    let idempotency_key = match &request {
        ReplayWebhookDeliveryRequest::ByIds {
            delivery_ids,
            idempotency_key,
        } => {
            if delivery_ids.len() > MAX_IDS_PER_REPLAY {
                return Err(replay_bad_request(
                    ERR_CODE_REPLAY_MAX_IDS_EXCEEDED,
                    format!(
                        "Maximum {} delivery IDs are allowed per replay",
                        MAX_IDS_PER_REPLAY
                    ),
                ));
            }
            idempotency_key.clone()
        }
        ReplayWebhookDeliveryRequest::ByDateRange {
            start_date,
            end_date,
            idempotency_key,
            status,
            event_name: _,
            endpoint_id,
        } => {
            let end = end_date.unwrap_or(now);
            if end < *start_date {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "end_date must be greater than or equal to start_date",
                )
                    .into());
            }
            if end.signed_duration_since(*start_date).num_seconds() > MAX_REPLAY_WINDOW_HOURS * 3600
            {
                return Err(replay_bad_request(
                    ERR_CODE_REPLAY_DATE_WINDOW_EXCEEDED,
                    "Replay range cannot exceed 48 hours",
                ));
            }
            if let Some(status_value) = status {
                if !validate_replay_status(status_value) {
                    return Err((StatusCode::BAD_REQUEST, "invalid status").into());
                }
            }
            if let Some(endpoint_id_value) = endpoint_id {
                endpoint_id_value
                    .parse::<i64>()
                    .map_err(|_| (StatusCode::BAD_REQUEST, "invalid endpoint_id"))?;
            }
            idempotency_key.clone()
        }
    };

    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to connect Redis: {}", e),
            )
        })?;

    let effective_idempotency_key = if let Some(raw_key) = idempotency_key {
        let trimmed = raw_key.trim().to_string();
        if trimmed.is_empty() {
            format!(
                "auto_{}",
                app_state
                    .sf
                    .next_id()
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            )
        } else {
            trimmed
        }
    } else {
        format!(
            "auto_{}",
            app_state
                .sf
                .next_id()
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        )
    };

    let redis_key = replay_idempotency_key(&app_slug, &effective_idempotency_key);
    let active_count_key = replay_active_count_key(&app_slug);
    let pending = "pending".to_string();
    let (exists, existing_value): (i32, String) = reserve_replay_slot(
        &mut redis_conn,
        &redis_key,
        &active_count_key,
        &pending,
        REPLAY_IDEMPOTENCY_TTL_SECS,
        MAX_ACTIVE_REPLAY_TASKS,
        REPLAY_ACTIVE_COUNT_TTL_SECS,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if exists == RESERVE_RESULT_EXISTS {
        let (_state, task_id, _ignored_hash) = parse_replay_idempotency_value(&existing_value);
        if let Some(existing_task_id) = task_id {
            return Ok(ReplayWebhookDeliveryResponse {
                status: "queued".to_string(),
                message: "Replay already queued for this idempotency key".to_string(),
                task_id: Some(existing_task_id),
            }
            .into());
        }
        return Ok(ReplayWebhookDeliveryResponse {
            status: "queued".to_string(),
            message: "Replay request is already being queued for this idempotency key".to_string(),
            task_id: None,
        }
        .into());
    }

    if exists == RESERVE_RESULT_LIMIT {
        return Err(replay_bad_request(
            ERR_CODE_REPLAY_CONCURRENCY_EXCEEDED,
            "Maximum 3 replay jobs can run at once for this app",
        ));
    }

    // Create strongly typed task payload based on request type
    let task_payload = match request {
        ReplayWebhookDeliveryRequest::ByIds {
            delivery_ids,
            idempotency_key: _,
        } => WebhookReplayBatchPayload::ByIds {
            deployment_id: deployment_id.to_string(),
            app_slug: app_slug.clone(),
            delivery_ids,
        },
        ReplayWebhookDeliveryRequest::ByDateRange {
            start_date,
            end_date,
            idempotency_key: _,
            status,
            event_name,
            endpoint_id,
        } => WebhookReplayBatchPayload::ByDateRange {
            deployment_id: deployment_id.to_string(),
            app_slug: app_slug.clone(),
            start_date,
            end_date,
            status,
            event_name,
            endpoint_id: endpoint_id.and_then(|value| value.parse::<i64>().ok()),
        },
    };

    let task_payload_json = match serde_json::to_value(task_payload) {
        Ok(value) => value,
        Err(e) => {
            let _ = rollback_replay_slot(&mut redis_conn, &redis_key, &active_count_key, &pending)
                .await;
            return Err((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize task payload: {}", e),
            )
                .into());
        }
    };

    let task_id = format!(
        "webhook-replay-batch-{}-{}",
        deployment_id,
        chrono::Utc::now().timestamp_millis()
    );
    let task_message = NatsTaskMessage {
        task_type: "webhook.replay_batch".to_string(),
        task_id: task_id.clone(),
        payload: task_payload_json,
    };
    let task_bytes = match serde_json::to_vec(&task_message) {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = rollback_replay_slot(&mut redis_conn, &redis_key, &active_count_key, &pending)
                .await;
            return Err((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize task: {}", e),
            )
                .into());
        }
    };

    // Queue to NATS for background processing
    if let Err(e) = app_state
        .nats_client
        .publish("worker.tasks.webhook.replay_batch", task_bytes.into())
        .await
    {
        let _ =
            rollback_replay_slot(&mut redis_conn, &redis_key, &active_count_key, &pending).await;
        return Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to queue replay task: {}", e),
        )
            .into());
    }

    let snapshot_key = replay_task_snapshot_key(&app_slug, &task_id);
    let index_key = replay_task_index_key(&app_slug);
    let now = chrono::Utc::now();

    let mut pipe = redis::pipe();
    pipe.atomic()
        .hset(&snapshot_key, "task_id", &task_id)
        .hset(&snapshot_key, "app_slug", &app_slug)
        .hset(&snapshot_key, "deployment_id", deployment_id)
        .hset(&snapshot_key, "status", "queued")
        .hset(&snapshot_key, "created_at", now.to_rfc3339())
        .hset(&snapshot_key, "processed_count", 0_i64)
        .hset(&snapshot_key, "replayed_count", 0_i64)
        .hset(&snapshot_key, "failed_count", 0_i64)
        .hset(&snapshot_key, "active_slot_reserved", "1")
        .expire(&snapshot_key, 86400)
        .zadd(&index_key, &task_id, now.timestamp())
        .expire(&index_key, 86400);
    if let Err(e) = pipe.query_async::<()>(&mut redis_conn).await {
        let _ =
            rollback_replay_slot(&mut redis_conn, &redis_key, &active_count_key, &pending).await;
        return Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to persist replay task snapshot: {}", e),
        )
            .into());
    }

    let final_value = format!("task:{}", task_id);
    let finalize_result: i32 = finalize_replay_idempotency(
        &mut redis_conn,
        &redis_key,
        &pending,
        &final_value,
        REPLAY_IDEMPOTENCY_TTL_SECS,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to finalize replay idempotency key: {}", e),
        )
    })?;
    if finalize_result != 1 {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to finalize replay idempotency key".to_string(),
        )
            .into());
    }

    Ok(ReplayWebhookDeliveryResponse {
        status: "queued".to_string(),
        message: "Webhook deliveries queued for replay".to_string(),
        task_id: Some(task_id),
    }
    .into())
}

pub async fn get_webhook_replay_task_status(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, task_id)): Path<(String, String)>,
) -> ApiResult<ReplayTaskStatusResponse> {
    GetWebhookAppByNameQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to connect Redis for replay task status: {}", e),
            )
        })?;

    let snapshot_key = replay_task_snapshot_key(&app_slug, &task_id);
    let data: HashMap<String, String> =
        redis_conn.hgetall(&snapshot_key).await.map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read replay task status: {}", e),
            )
        })?;

    if data.is_empty() {
        return Err((StatusCode::NOT_FOUND, "Replay task not found").into());
    }

    Ok(ReplayTaskStatusResponse {
        task_id,
        app_slug,
        status: data
            .get("status")
            .cloned()
            .unwrap_or_else(|| "queued".to_string()),
        created_at: data.get("created_at").cloned(),
        started_at: data.get("started_at").cloned(),
        completed_at: data.get("completed_at").cloned(),
        total_count: parse_replay_i64(&data, "total_count"),
        processed: parse_replay_i64(&data, "processed_count"),
        replayed_count: parse_replay_i64(&data, "replayed_count"),
        failed_count: parse_replay_i64(&data, "failed_count"),
        last_delivery_id: {
            let v = parse_replay_i64(&data, "last_delivery_id");
            if v > 0 { Some(v) } else { None }
        },
    }
    .into())
}

pub async fn cancel_webhook_replay_task(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, task_id)): Path<(String, String)>,
) -> ApiResult<ReplayTaskCancelResponse> {
    let app = GetWebhookAppByNameQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let snapshot_key = replay_task_snapshot_key(&app.app_slug, &task_id);

    let exists: i32 = redis_conn
        .exists(&snapshot_key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if exists == 0 {
        return Err((StatusCode::NOT_FOUND, "Replay task not found").into());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let active_count_key = replay_active_count_key(&app.app_slug);
    let _: i32 = cancel_replay_task(
        &mut redis_conn,
        &snapshot_key,
        &active_count_key,
        &now,
        7200_i64,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(ReplayTaskCancelResponse {
        status: "cancelled".to_string(),
        message: "Replay task cancellation requested".to_string(),
    }
    .into())
}

pub async fn list_webhook_replay_tasks(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ReplayTaskListQuery>,
) -> ApiResult<ReplayTaskListResponse> {
    GetWebhookAppByNameQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let offset = params.offset.unwrap_or(0).max(0);

    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to connect Redis for replay task list: {}", e),
            )
        })?;

    let task_ids: Vec<String> = redis_conn
        .zrevrange(
            replay_task_index_key(&app_slug),
            offset as isize,
            (offset + limit) as isize,
        )
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to fetch replay task list: {}", e),
            )
        })?;

    let paged_ids = paginate_results(task_ids, limit as i32, Some(offset as i64));
    let has_more = paged_ids.has_more;
    let ids = paged_ids.data;

    let mut data = Vec::with_capacity(ids.len());
    for task_id in ids {
        let snapshot_key = replay_task_snapshot_key(&app_slug, &task_id);
        let fields: HashMap<String, String> =
            redis_conn.hgetall(&snapshot_key).await.unwrap_or_default();
        if fields.is_empty() {
            continue;
        }
        data.push(ReplayTaskStatusResponse {
            task_id,
            app_slug: app_slug.clone(),
            status: fields
                .get("status")
                .cloned()
                .unwrap_or_else(|| "queued".to_string()),
            created_at: fields.get("created_at").cloned(),
            started_at: fields.get("started_at").cloned(),
            completed_at: fields.get("completed_at").cloned(),
            total_count: parse_replay_i64(&fields, "total_count"),
            processed: parse_replay_i64(&fields, "processed_count"),
            replayed_count: parse_replay_i64(&fields, "replayed_count"),
            failed_count: parse_replay_i64(&fields, "failed_count"),
            last_delivery_id: {
                let v = parse_replay_i64(&fields, "last_delivery_id");
                if v > 0 { Some(v) } else { None }
            },
        });
    }

    Ok(ReplayTaskListResponse {
        data,
        limit,
        offset,
        has_more,
    }
    .into())
}

fn replay_task_snapshot_key(app_slug: &str, task_id: &str) -> String {
    format!("worker:webhook:replay:{}:{}", app_slug, task_id)
}

fn replay_task_index_key(app_slug: &str) -> String {
    format!("worker:webhook:replay:index:{}", app_slug)
}

fn replay_active_count_key(app_slug: &str) -> String {
    format!("worker:webhook:replay:active_count:{}", app_slug)
}

fn replay_idempotency_key(app_slug: &str, idempotency_key: &str) -> String {
    format!(
        "worker:webhook:replay:idem:{}:{}",
        app_slug, idempotency_key
    )
}

fn parse_replay_idempotency_value(value: &str) -> (String, Option<String>, Option<String>) {
    if value == "pending" {
        return ("pending".to_string(), None, None);
    }
    if let Some(hash) = value.strip_prefix("pending:") {
        return ("pending".to_string(), None, Some(hash.to_string()));
    }
    if let Some(rest) = value.strip_prefix("task:") {
        let mut parts = rest.splitn(2, ':');
        if let Some(task_id) = parts.next() {
            if let Some(hash) = parts.next() {
                return (
                    "task".to_string(),
                    Some(task_id.to_string()),
                    Some(hash.to_string()),
                );
            }
            return ("task".to_string(), Some(task_id.to_string()), None);
        }
    }
    ("".to_string(), None, None)
}

fn parse_replay_i64(data: &HashMap<String, String>, key: &str) -> i64 {
    data.get(key)
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
}

async fn rollback_replay_slot(
    redis_conn: &mut redis::aio::MultiplexedConnection,
    idempotency_key: &str,
    active_count_key: &str,
    pending_value: &str,
) -> redis::RedisResult<()> {
    let rollback_script = Script::new(LUA_REPLAY_ROLLBACK_SLOT);
    let _: i32 = rollback_script
        .key(idempotency_key)
        .key(active_count_key)
        .arg(pending_value)
        .invoke_async(redis_conn)
        .await?;
    Ok(())
}

async fn reserve_replay_slot(
    redis_conn: &mut redis::aio::MultiplexedConnection,
    idempotency_key: &str,
    active_count_key: &str,
    pending_value: &str,
    idempotency_ttl_secs: i64,
    max_active_replay_tasks: i32,
    active_count_ttl_secs: i64,
) -> redis::RedisResult<(i32, String)> {
    let reserve_script = Script::new(LUA_REPLAY_RESERVE);
    reserve_script
        .key(idempotency_key)
        .key(active_count_key)
        .arg(pending_value)
        .arg(idempotency_ttl_secs)
        .arg(max_active_replay_tasks)
        .arg(active_count_ttl_secs)
        .invoke_async(redis_conn)
        .await
}

async fn finalize_replay_idempotency(
    redis_conn: &mut redis::aio::MultiplexedConnection,
    idempotency_key: &str,
    pending_value: &str,
    final_value: &str,
    idempotency_ttl_secs: i64,
) -> redis::RedisResult<i32> {
    let finalize_script = Script::new(LUA_REPLAY_FINALIZE);
    finalize_script
        .key(idempotency_key)
        .arg(pending_value)
        .arg(final_value)
        .arg(idempotency_ttl_secs)
        .invoke_async(redis_conn)
        .await
}

async fn cancel_replay_task(
    redis_conn: &mut redis::aio::MultiplexedConnection,
    snapshot_key: &str,
    active_count_key: &str,
    now_rfc3339: &str,
    ttl_secs: i64,
) -> redis::RedisResult<i32> {
    let cancel_script = Script::new(LUA_REPLAY_CANCEL);
    cancel_script
        .key(snapshot_key)
        .key(active_count_key)
        .arg(now_rfc3339)
        .arg(ttl_secs)
        .invoke_async(redis_conn)
        .await
}
