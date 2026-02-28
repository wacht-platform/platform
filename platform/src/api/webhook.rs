use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use chrono::{Datelike, Utc};
use models::webhook_analytics::{WebhookAnalyticsResult, WebhookTimeseriesResult};
use queries::GetWebhookAppByNameQuery;
use queries::webhook_analytics::{GetWebhookAnalyticsQuery, GetWebhookTimeseriesQuery};
use redis::{AsyncCommands, Script};

use crate::application::response::{ApiError, ApiErrorResponse, ApiResult, PaginatedResponse};
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    webhook_app::{
        CreateWebhookAppCommand, DeleteWebhookAppCommand, RotateWebhookSecretCommand,
        UpdateWebhookAppCommand,
    },
    webhook_endpoint::{
        CreateWebhookEndpointCommand, DeleteWebhookEndpointCommand, UpdateWebhookEndpointCommand,
    },
    webhook_event_catalog::{CreateEventCatalogCommand, UpdateEventCatalogCommand},
    webhook_trigger::TriggerWebhookEventCommand,
};
use common::state::AppState;
use dto::clickhouse::webhook::{WebhookDeliveryListResponse, WebhookLog};
use dto::json::webhook_requests::{WebhookEndpoint as WebhookEndpointDto, *};
use models::webhook::{WebhookApp, WebhookEndpoint};
use queries::{
    Query as QueryTrait,
    webhook::{
        GetWebhookAppsQuery, GetWebhookEndpointsWithSubscriptionsQuery, GetWebhookEventsQuery,
    },
};

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

pub async fn list_webhook_apps(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookAppsQuery>,
) -> ApiResult<PaginatedResponse<WebhookApp>> {
    let include_inactive = params.include_inactive.unwrap_or(false);
    let limit = params.limit.unwrap_or(50) as u64;
    let offset = params.offset.unwrap_or(0) as u64;

    let mut apps = GetWebhookAppsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .with_pagination(Some(limit as i64 + 1), Some(offset as i64))
        .execute(&app_state)
        .await?;

    let has_more = apps.len() > limit as usize;
    if has_more {
        apps.pop();
    }

    Ok(PaginatedResponse {
        data: apps,
        has_more,
        limit: Some(limit as i32),
        offset: Some(offset as i32),
    }
    .into())
}

pub async fn create_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookAppRequest>,
) -> ApiResult<WebhookApp> {
    let mut command = CreateWebhookAppCommand::new(deployment_id, request.name);

    if let Some(description) = request.description {
        command = command.with_description(description);
    }

    if let Some(emails) = request.failure_notification_emails {
        command = command.with_failure_notification_emails(emails);
    }

    if let Some(catalog_slug) = request.event_catalog_slug {
        command = command.with_event_catalog_slug(catalog_slug);
    }

    let app = command.execute(&app_state).await?;

    Ok(app.into())
}

pub async fn list_event_catalogs(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<models::webhook::WebhookEventCatalog>> {
    let catalogs: Vec<models::webhook::WebhookEventCatalog> =
        commands::webhook_event_catalog::ListEventCatalogsQuery::new(deployment_id)
            .execute(&app_state)
            .await?;

    Ok(PaginatedResponse {
        data: catalogs,
        has_more: false,
        limit: None,
        offset: None,
    }
    .into())
}

pub async fn create_event_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateEventCatalogRequest>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let command = CreateEventCatalogCommand {
        deployment_id,
        slug: request.slug,
        name: request.name,
        description: request.description,
        events: request.events,
    };

    let catalog: models::webhook::WebhookEventCatalog = command.execute(&app_state).await?;

    Ok(catalog.into())
}

pub async fn get_event_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let catalog: Option<models::webhook::WebhookEventCatalog> =
        commands::webhook_event_catalog::GetEventCatalogQuery::new(deployment_id, slug)
            .execute(&app_state)
            .await?;

    let catalog =
        catalog.ok_or_else(|| (StatusCode::NOT_FOUND, "Event catalog not found".to_string()))?;

    Ok(catalog.into())
}

pub async fn update_event_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<UpdateEventCatalogRequest>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let command = UpdateEventCatalogCommand {
        deployment_id,
        slug,
        name: request.name,
        description: request.description,
    };

    let catalog: models::webhook::WebhookEventCatalog = command.execute(&app_state).await?;

    Ok(catalog.into())
}

pub async fn append_events_to_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<AppendEventsToCatalogRequest>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let command = commands::webhook_event_catalog::AppendEventsToCatalogCommand {
        deployment_id,
        slug,
        events: request.events,
    };

    let catalog = command.execute(&app_state).await?;

    Ok(catalog.into())
}

pub async fn archive_event_in_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<ArchiveEventInCatalogRequest>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let command = commands::webhook_event_catalog::ArchiveEventInCatalogCommand {
        deployment_id,
        slug,
        event_name: request.event_name,
        is_archived: request.is_archived,
    };

    let catalog = command.execute(&app_state).await?;

    Ok(catalog.into())
}

pub async fn update_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(request): Json<UpdateWebhookAppRequest>,
) -> ApiResult<WebhookApp> {
    let command = UpdateWebhookAppCommand {
        deployment_id,
        app_slug,
        new_name: request.name,
        description: request.description,
        is_active: request.is_active,
        failure_notification_emails: request.failure_notification_emails,
        event_catalog_slug: request.event_catalog_slug,
    };

    let app: WebhookApp = command.execute(&app_state).await?;
    Ok(app.into())
}

pub async fn get_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<WebhookApp> {
    let command = GetWebhookAppByNameQuery::new(deployment_id, app_slug);

    let app = command
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    Ok(app.into())
}

pub async fn delete_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<()> {
    let command = DeleteWebhookAppCommand {
        deployment_id,
        app_slug,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn rotate_webhook_secret(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<WebhookApp> {
    let command = RotateWebhookSecretCommand {
        deployment_id,
        app_slug,
    };
    let app = command.execute(&app_state).await?;

    Ok(app.into())
}

pub async fn get_webhook_events(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<GetAvailableEventsResponse> {
    let model_events = GetWebhookEventsQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?;

    // Convert model events to SDK format
    let events: Vec<wacht::api::webhooks::WebhookAppEvent> = model_events
        .into_iter()
        .map(|e| wacht::api::webhooks::WebhookAppEvent {
            deployment_id: deployment_id.to_string(),
            app_slug: app_slug.clone(),
            event_name: e.name,
            description: Some(e.description),
            schema: e.schema,
            created_at: chrono::Utc::now(), // Best effort for catalog-based events if not stored per-event
        })
        .collect();

    Ok(GetAvailableEventsResponse { events }.into())
}

pub async fn get_webhook_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let app = GetWebhookAppByNameQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    let catalog_slug = app.event_catalog_slug.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "No catalog assigned to this app".to_string(),
        )
    })?;

    let catalog =
        commands::webhook_event_catalog::GetEventCatalogQuery::new(deployment_id, catalog_slug)
            .execute(&app_state)
            .await?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Event catalog not found".to_string()))?;

    Ok(catalog.into())
}

pub async fn list_webhook_endpoints(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<PaginatedResponse<WebhookEndpointDto>> {
    let include_inactive = params.include_inactive.unwrap_or(false);
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    // Fetch one extra to determine if there are more
    // The query already returns dto::json::webhook_requests::WebhookEndpoint with subscriptions
    let mut endpoints = GetWebhookEndpointsWithSubscriptionsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .for_app(app_slug)
        .with_pagination(Some(limit + 1), Some(offset))
        .execute(&app_state)
        .await?;

    let has_more = endpoints.len() > limit as usize;
    if has_more {
        endpoints.truncate(limit as usize);
    }

    Ok(PaginatedResponse {
        data: endpoints,
        has_more,
        limit: Some(limit),
        offset: Some(offset),
    }
    .into())
}

pub async fn create_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    use commands::webhook_endpoint::EventSubscriptionData;
    let rate_limit_config = request
        .rate_limit_config
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid rate_limit_config: {}", e),
            )
        })?;

    // Convert API subscriptions to command subscriptions
    let subscriptions: Vec<EventSubscriptionData> = request
        .subscriptions
        .into_iter()
        .map(|sub| EventSubscriptionData {
            event_name: sub.event_name,
            filter_rules: sub.filter_rules,
        })
        .collect();

    let command = CreateWebhookEndpointCommand {
        deployment_id,
        app_slug: request.app_slug,
        url: request.url,
        description: request.description,
        headers: request.headers,
        subscriptions,
        max_retries: request.max_retries,
        timeout_seconds: request.timeout_seconds,
        rate_limit_config,
    };

    let endpoint = command.execute(&app_state).await?;

    Ok(endpoint.into())
}

pub async fn create_webhook_endpoint_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(mut request): Json<CreateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    request.app_slug = app_slug;
    create_webhook_endpoint(
        State(app_state),
        RequireDeployment(deployment_id),
        Json(request),
    )
    .await
}

pub async fn update_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let rate_limit_config = request
        .rate_limit_config
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid rate_limit_config: {}", e),
            )
        })?;

    let command = UpdateWebhookEndpointCommand {
        endpoint_id,
        deployment_id,
        url: request.url,
        description: request.description,
        headers: request.headers,
        max_retries: request.max_retries,
        timeout_seconds: request.timeout_seconds,
        is_active: request.is_active,
        subscriptions: request
            .subscriptions
            .map(|subs| subs.into_iter().map(Into::into).collect()),
        rate_limit_config,
    };

    let endpoint = command.execute(&app_state).await?;
    Ok(endpoint.into())
}

pub async fn update_webhook_endpoint_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, endpoint_id)): Path<(String, i64)>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let endpoints = queries::webhook::GetWebhookEndpointsQuery::new(deployment_id)
        .for_app(app_slug)
        .with_inactive(true)
        .execute(&app_state)
        .await?;
    if !endpoints.iter().any(|e| e.id == endpoint_id) {
        return Err((StatusCode::NOT_FOUND, "Webhook endpoint not found").into());
    }

    update_webhook_endpoint(
        State(app_state),
        RequireDeployment(deployment_id),
        Path(endpoint_id),
        Json(request),
    )
    .await
}

pub async fn delete_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<()> {
    let command = DeleteWebhookEndpointCommand {
        endpoint_id,
        deployment_id,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn delete_webhook_endpoint_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, endpoint_id)): Path<(String, i64)>,
) -> ApiResult<()> {
    let endpoints = queries::webhook::GetWebhookEndpointsQuery::new(deployment_id)
        .for_app(app_slug)
        .with_inactive(true)
        .execute(&app_state)
        .await?;
    if !endpoints.iter().any(|e| e.id == endpoint_id) {
        return Err((StatusCode::NOT_FOUND, "Webhook endpoint not found").into());
    }

    delete_webhook_endpoint(
        State(app_state),
        RequireDeployment(deployment_id),
        Path(endpoint_id),
    )
    .await
}

pub async fn trigger_webhook_event(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(request): Json<TriggerWebhookEventRequest>,
) -> ApiResult<TriggerWebhookEventResponse> {
    let mut command = TriggerWebhookEventCommand::new(
        deployment_id,
        app_slug,
        request.event_name,
        request.payload,
    );

    if let Some(context) = request.filter_context {
        command = command.with_filter_context(context);
    }

    let result = command.execute(&app_state).await?;

    tokio::spawn({
        let redis = app_state.redis_client.clone();
        async move {
            if let Ok(mut conn) = redis.get_multiplexed_async_connection().await {
                let now = Utc::now();
                let period = format!("{}-{:02}", now.year(), now.month());
                let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

                let mut pipe = redis::pipe();
                pipe.atomic()
                    .zincr(&format!("{}:metrics", prefix), "webhooks", 1)
                    .ignore()
                    .expire(&format!("{}:metrics", prefix), 5184000)
                    .ignore()
                    .zincr(
                        &format!("billing:{}:dirty_deployments", period),
                        deployment_id,
                        1,
                    )
                    .ignore()
                    .expire(&format!("billing:{}:dirty_deployments", period), 5184000)
                    .ignore();

                let _: Result<(), redis::RedisError> = pipe.query_async(&mut conn).await;
            }
        }
    });

    Ok(TriggerWebhookEventResponse {
        delivery_ids: result.delivery_ids,
        filtered_count: result.filtered_count,
        delivered_count: result.delivered_count,
    }
    .into())
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
    let data: std::collections::HashMap<String, String> =
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

    let has_more = task_ids.len() > limit as usize;
    let ids = if has_more {
        task_ids[..limit as usize].to_vec()
    } else {
        task_ids
    };

    let mut data = Vec::with_capacity(ids.len());
    for task_id in ids {
        let snapshot_key = replay_task_snapshot_key(&app_slug, &task_id);
        let fields: std::collections::HashMap<String, String> =
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

fn parse_replay_i64(data: &std::collections::HashMap<String, String>, key: &str) -> i64 {
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

pub async fn get_webhook_delivery_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(delivery_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<dto::json::webhook_requests::WebhookDeliveryDetails> {
    let delivery_id = delivery_id
        .parse::<i64>()
        .map_err(|_| (axum::http::StatusCode::BAD_REQUEST, "Invalid delivery ID"))?;

    // Check if status=pending to look in PostgreSQL instead of ClickHouse
    let status = params.get("status").map(|s| s.as_str());

    let delivery = if status == Some("pending") {
        // Check PostgreSQL for active/pending deliveries
        queries::webhook::GetPendingWebhookDeliveryQuery::new(deployment_id, delivery_id)
            .execute(&app_state)
            .await?
    } else {
        // Check ClickHouse for completed deliveries
        app_state
            .clickhouse_service
            .get_webhook_delivery_details(deployment_id, delivery_id)
            .await?
    };

    // Parse payload from storage JSON string to structured JSON response
    let payload = delivery
        .payload
        .clone()
        .and_then(|p| serde_json::from_str(&p).ok());

    // Convert WebhookDelivery to WebhookDeliveryDetails
    let delivery_details = dto::json::webhook_requests::WebhookDeliveryDetails {
        delivery_id: delivery.delivery_id,
        deployment_id: delivery.deployment_id,
        app_slug: delivery.app_slug,
        endpoint_id: delivery.endpoint_id,
        event_name: delivery.event_name,
        status: delivery.status,
        http_status_code: delivery.http_status_code,
        response_time_ms: delivery.response_time_ms,
        attempt_number: delivery.attempt_number,
        max_attempts: delivery.max_attempts,
        payload,
        response_body: delivery.response_body,
        response_headers: delivery
            .response_headers
            .and_then(|h| serde_json::from_str(&h).ok()),
        timestamp: delivery.timestamp,
    };

    Ok(delivery_details.into())
}

pub async fn get_webhook_delivery_details_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, delivery_id)): Path<(String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<dto::json::webhook_requests::WebhookDeliveryDetails> {
    GetWebhookAppByNameQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    get_webhook_delivery_details(
        State(app_state),
        RequireDeployment(deployment_id),
        Path(delivery_id),
        Query(params),
    )
    .await
}

pub async fn reactivate_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<ReactivateEndpointResponse> {
    use commands::webhook_endpoint::ReactivateEndpointCommand;

    // Reactivate the endpoint and clear failure counter
    let endpoint = ReactivateEndpointCommand {
        endpoint_id,
        deployment_id,
    }
    .execute(&app_state)
    .await?;

    // Log reactivation to Tinybird
    if let Ok(log_id) = app_state.sf.next_id() {
        let ch_log = WebhookLog {
            deployment_id,
            delivery_id: log_id as i64,
            app_slug: endpoint.app_slug.clone(),
            endpoint_id: endpoint.id,
            event_name: "endpoint.reactivated".to_string(),
            status: "reactivated".to_string(),
            http_status_code: None,
            response_time_ms: None,
            attempt_number: 0,
            max_attempts: 1,
            payload: None,
            payload_size_bytes: 0,
            response_body: None,
            response_headers: None,
            request_headers: None,
            timestamp: chrono::Utc::now(),
        };

        let _ = app_state
            .clickhouse_service
            .insert_webhook_log(&ch_log)
            .await;
    } else {
        tracing::warn!("Failed to generate snowflake id for webhook reactivation log");
    }

    Ok(ReactivateEndpointResponse {
        success: true,
        message: format!("Endpoint {} reactivated successfully", endpoint.url),
    }
    .into())
}

pub async fn test_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((_app_name, endpoint_id)): Path<(String, i64)>,
    Json(request): Json<TestWebhookEndpointRequest>,
) -> ApiResult<TestWebhookEndpointResponse> {
    use commands::webhook_endpoint::TestWebhookEndpointCommand;

    // Use the payload from the request
    let test_payload = request.payload.unwrap_or_else(|| {
        serde_json::json!({
            "test": true,
            "event": request.event_name,
            "timestamp": chrono::Utc::now()
        })
    });

    let result = TestWebhookEndpointCommand {
        endpoint_id,
        deployment_id,
        test_payload,
    }
    .execute(&app_state)
    .await?;

    Ok(TestWebhookEndpointResponse {
        success: result.success,
        status_code: result.status_code,
        response_time_ms: result.response_time_ms,
        response_body: result.response_body,
        error: result.error,
    }
    .into())
}

pub async fn get_webhook_analytics(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<WebhookAnalyticsQuery>,
) -> ApiResult<WebhookAnalyticsResult> {
    let mut query = GetWebhookAnalyticsQuery::new(deployment_id).with_app_slug(app_slug);

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}

pub async fn get_webhook_timeseries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<WebhookTimeseriesQuery>,
) -> ApiResult<WebhookTimeseriesResult> {
    let mut query =
        GetWebhookTimeseriesQuery::new(deployment_id, params.interval).with_app_slug(app_slug);

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}

pub async fn get_webhook_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<WebhookAnalyticsResult> {
    let query = GetWebhookAnalyticsQuery::new(deployment_id).with_app_slug(app_slug);

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}

pub async fn get_app_webhook_deliveries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetAppWebhookDeliveriesQuery>,
) -> ApiResult<PaginatedResponse<WebhookDeliveryListResponse>> {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    // Fetch one extra to determine if there are more
    let delivery_rows = app_state
        .clickhouse_service
        .get_webhook_deliveries(
            deployment_id,
            Some(app_slug.clone()),
            params.status.as_deref(),
            params.event_name.as_deref(),
            (limit + 1) as usize,
            offset as usize,
        )
        .await?;

    let has_more = delivery_rows.len() > limit as usize;
    let mut deliveries: Vec<WebhookDeliveryListResponse> =
        delivery_rows.into_iter().map(|row| row.into()).collect();

    if has_more {
        deliveries.truncate(limit as usize);
    }

    Ok(PaginatedResponse {
        data: deliveries,
        has_more,
        limit: Some(limit),
        offset: Some(offset),
    }
    .into())
}
