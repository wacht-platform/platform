use super::*;

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

    let command = CreateWebhookEndpointCommand::new(deployment_id, request.app_slug, request.url)
        .with_description(request.description)
        .with_headers(request.headers)
        .with_subscriptions(subscriptions)
        .with_max_retries(request.max_retries)
        .with_timeout_seconds(request.timeout_seconds)
        .with_rate_limit_config(rate_limit_config);

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

    let command = UpdateWebhookEndpointCommand::new(endpoint_id, deployment_id)
        .with_url(request.url)
        .with_description(request.description)
        .with_headers(request.headers)
        .with_max_retries(request.max_retries)
        .with_timeout_seconds(request.timeout_seconds)
        .with_is_active(request.is_active)
        .with_subscriptions(
            request
                .subscriptions
                .map(|subs| subs.into_iter().map(Into::into).collect()),
        )
        .with_rate_limit_config(rate_limit_config);

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
    let command = DeleteWebhookEndpointCommand::new(endpoint_id, deployment_id);
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

pub async fn reactivate_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<ReactivateEndpointResponse> {
    // Reactivate the endpoint and clear failure counter
    let endpoint = ReactivateEndpointCommand::new(endpoint_id, deployment_id)
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
    // Use the payload from the request
    let test_payload = request.payload.unwrap_or_else(|| {
        serde_json::json!({
            "test": true,
            "event": request.event_name,
            "timestamp": chrono::Utc::now()
        })
    });

    let result = TestWebhookEndpointCommand::new(endpoint_id, deployment_id, test_payload)
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
