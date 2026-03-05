use super::*;

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
