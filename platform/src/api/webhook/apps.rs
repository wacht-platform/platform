use axum::{
    Json,
    extract::{Path, Query, State},
};
use common::state::AppState;
use dto::json::webhook_requests::{
    AppendEventsToCatalogRequest, ArchiveEventInCatalogRequest, CreateEventCatalogRequest,
    CreateWebhookAppRequest, GetAvailableEventsResponse, ListWebhookAppsQuery,
    UpdateEventCatalogRequest, UpdateWebhookAppRequest,
};
use models::webhook::WebhookApp;

use crate::application::{
    response::{ApiResult, PaginatedResponse},
    webhook_apps as webhook_apps_app,
};
use crate::middleware::RequireDeployment;

pub async fn list_webhook_apps(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookAppsQuery>,
) -> ApiResult<PaginatedResponse<WebhookApp>> {
    let apps = webhook_apps_app::list_webhook_apps(&app_state, deployment_id, params).await?;
    Ok(apps.into())
}

pub async fn create_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookAppRequest>,
) -> ApiResult<WebhookApp> {
    let app =
        webhook_apps_app::create_webhook_app(&app_state, deployment_id, request).await?;
    Ok(app.into())
}

pub async fn list_event_catalogs(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<models::webhook::WebhookEventCatalog>> {
    let catalogs = webhook_apps_app::list_event_catalogs(&app_state, deployment_id).await?;
    Ok(catalogs.into())
}

pub async fn create_event_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateEventCatalogRequest>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let catalog =
        webhook_apps_app::create_event_catalog(&app_state, deployment_id, request).await?;
    Ok(catalog.into())
}

pub async fn get_event_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let catalog =
        webhook_apps_app::get_event_catalog(&app_state, deployment_id, slug).await?;
    Ok(catalog.into())
}

pub async fn update_event_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<UpdateEventCatalogRequest>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let catalog =
        webhook_apps_app::update_event_catalog(&app_state, deployment_id, slug, request)
            .await?;
    Ok(catalog.into())
}

pub async fn append_events_to_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<AppendEventsToCatalogRequest>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let catalog =
        webhook_apps_app::append_events_to_catalog(&app_state, deployment_id, slug, request)
            .await?;
    Ok(catalog.into())
}

pub async fn archive_event_in_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<ArchiveEventInCatalogRequest>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let catalog =
        webhook_apps_app::archive_event_in_catalog(&app_state, deployment_id, slug, request)
            .await?;
    Ok(catalog.into())
}

pub async fn update_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(request): Json<UpdateWebhookAppRequest>,
) -> ApiResult<WebhookApp> {
    let app =
        webhook_apps_app::update_webhook_app(&app_state, deployment_id, app_slug, request)
            .await?;
    Ok(app.into())
}

pub async fn get_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<WebhookApp> {
    let app = webhook_apps_app::get_webhook_app(&app_state, deployment_id, app_slug).await?;
    Ok(app.into())
}

pub async fn delete_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<()> {
    webhook_apps_app::delete_webhook_app(&app_state, deployment_id, app_slug).await?;
    Ok(().into())
}

pub async fn rotate_webhook_secret(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<WebhookApp> {
    let app =
        webhook_apps_app::rotate_webhook_secret(&app_state, deployment_id, app_slug).await?;
    Ok(app.into())
}

pub async fn get_webhook_events(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<GetAvailableEventsResponse> {
    let events =
        webhook_apps_app::get_webhook_events(&app_state, deployment_id, app_slug).await?;
    Ok(events.into())
}

pub async fn get_webhook_catalog(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<models::webhook::WebhookEventCatalog> {
    let catalog = webhook_apps_app::get_webhook_catalog(&app_state, deployment_id, app_slug)
        .await
        .map_err(webhook_apps_app::map_app_error_to_api)?;

    Ok(catalog.into())
}
