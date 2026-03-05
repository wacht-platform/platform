use super::*;

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

