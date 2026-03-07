use axum::http::StatusCode;
use commands::{
    webhook_app::{
        CreateWebhookAppCommand, DeleteWebhookAppCommand, RotateWebhookSecretCommand,
        UpdateWebhookAppCommand,
    },
    webhook_event_catalog::{CreateEventCatalogCommand, UpdateEventCatalogCommand},
};
use common::ReadConsistency;
use common::error::AppError;
use common::state::AppState;
use dto::json::webhook_requests::{
    AppendEventsToCatalogRequest, ArchiveEventInCatalogRequest, CreateEventCatalogRequest,
    CreateWebhookAppRequest, GetAvailableEventsResponse, ListWebhookAppsQuery,
    UpdateEventCatalogRequest, UpdateWebhookAppRequest,
};
use models::webhook::WebhookApp;
use queries::{
    GetWebhookAppByNameQuery,
    webhook::{GetWebhookAppsQuery, GetWebhookEventsQuery},
};

use crate::{api::pagination::paginate_results, application::response::PaginatedResponse};

pub async fn list_webhook_apps(
    app_state: &AppState,
    deployment_id: i64,
    params: ListWebhookAppsQuery,
) -> Result<PaginatedResponse<WebhookApp>, AppError> {
    let include_inactive = params.include_inactive.unwrap_or(false);
    let limit = params.limit.unwrap_or(50) as u64;
    let offset = params.offset.unwrap_or(0) as u64;

    let apps = GetWebhookAppsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .with_pagination(Some(limit as i64 + 1), Some(offset as i64))
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(paginate_results(apps, limit as i32, Some(offset as i64)))
}

pub async fn create_webhook_app(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateWebhookAppRequest,
) -> Result<WebhookApp, AppError> {
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

    command
        .with_generated_slug(format!("slug_{}", app_state.sf.next_id()?))
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn list_event_catalogs(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<PaginatedResponse<models::webhook::WebhookEventCatalog>, AppError> {
    let catalogs: Vec<models::webhook::WebhookEventCatalog> =
        commands::webhook_event_catalog::ListEventCatalogsQuery::new(deployment_id)
            .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
            .await?;

    Ok(PaginatedResponse::from(catalogs))
}

pub async fn create_event_catalog(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateEventCatalogRequest,
) -> Result<models::webhook::WebhookEventCatalog, AppError> {
    let command =
        CreateEventCatalogCommand::new(deployment_id, request.slug, request.name, request.events)
            .with_description(request.description);
    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn get_event_catalog(
    app_state: &AppState,
    deployment_id: i64,
    slug: String,
) -> Result<models::webhook::WebhookEventCatalog, AppError> {
    let catalog: Option<models::webhook::WebhookEventCatalog> =
        commands::webhook_event_catalog::GetEventCatalogQuery::new(deployment_id, slug)
            .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
            .await?;

    catalog.ok_or_else(|| AppError::NotFound("Event catalog not found".to_string()))
}

pub async fn update_event_catalog(
    app_state: &AppState,
    deployment_id: i64,
    slug: String,
    request: UpdateEventCatalogRequest,
) -> Result<models::webhook::WebhookEventCatalog, AppError> {
    let command = UpdateEventCatalogCommand::new(deployment_id, slug)
        .with_name(request.name)
        .with_description(request.description);
    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn append_events_to_catalog(
    app_state: &AppState,
    deployment_id: i64,
    slug: String,
    request: AppendEventsToCatalogRequest,
) -> Result<models::webhook::WebhookEventCatalog, AppError> {
    let command = commands::webhook_event_catalog::AppendEventsToCatalogCommand::new(
        deployment_id,
        slug,
        request.events,
    );
    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn archive_event_in_catalog(
    app_state: &AppState,
    deployment_id: i64,
    slug: String,
    request: ArchiveEventInCatalogRequest,
) -> Result<models::webhook::WebhookEventCatalog, AppError> {
    let command = commands::webhook_event_catalog::ArchiveEventInCatalogCommand::new(
        deployment_id,
        slug,
        request.event_name,
        request.is_archived,
    );
    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn update_webhook_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    request: UpdateWebhookAppRequest,
) -> Result<WebhookApp, AppError> {
    let command = UpdateWebhookAppCommand::new(deployment_id, app_slug)
        .with_new_name(request.name)
        .with_description(request.description)
        .with_is_active(request.is_active)
        .with_failure_notification_emails(request.failure_notification_emails)
        .with_event_catalog_slug(request.event_catalog_slug);
    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn get_webhook_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<WebhookApp, AppError> {
    GetWebhookAppByNameQuery::new(deployment_id, app_slug)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook app not found".to_string()))
}

pub async fn delete_webhook_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<(), AppError> {
    let command = DeleteWebhookAppCommand::new(deployment_id, app_slug);
    command.execute_with_db(app_state.db_router.writer()).await?;
    Ok(())
}

pub async fn rotate_webhook_secret(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<WebhookApp, AppError> {
    let command = RotateWebhookSecretCommand::new(deployment_id, app_slug);
    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn get_webhook_events(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<GetAvailableEventsResponse, AppError> {
    let model_events = GetWebhookEventsQuery::new(deployment_id, app_slug.clone())
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let events: Vec<wacht::api::webhooks::WebhookAppEvent> = model_events
        .into_iter()
        .map(|e| wacht::api::webhooks::WebhookAppEvent {
            deployment_id: deployment_id.to_string(),
            app_slug: app_slug.clone(),
            event_name: e.name,
            description: Some(e.description),
            schema: e.schema,
            created_at: chrono::Utc::now(),
        })
        .collect();

    Ok(GetAvailableEventsResponse { events })
}

pub async fn get_webhook_catalog(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<models::webhook::WebhookEventCatalog, AppError> {
    let app = get_webhook_app(app_state, deployment_id, app_slug).await?;

    let catalog_slug = app
        .event_catalog_slug
        .ok_or_else(|| AppError::NotFound("No catalog assigned to this app".to_string()))?;

    let catalog =
        commands::webhook_event_catalog::GetEventCatalogQuery::new(deployment_id, catalog_slug)
            .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
            .await?
            .ok_or_else(|| AppError::NotFound("Event catalog not found".to_string()))?;

    Ok(catalog)
}

pub fn map_app_error_to_api(err: AppError) -> crate::application::response::ApiErrorResponse {
    match err {
        AppError::NotFound(msg) if msg == "No catalog assigned to this app" => {
            (StatusCode::NOT_FOUND, msg).into()
        }
        other => other.into(),
    }
}
