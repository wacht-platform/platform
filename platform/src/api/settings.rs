use crate::{
    application::{
        response::{ApiResult, PaginatedResponse},
    },
    middleware::RequireDeployment,
};
use common::state::AppState;

use commands::{
    Command, CreateDeploymentJwtTemplateCommand, DeleteDeploymentJwtTemplateCommand,
    UpdateDeploymentAuthSettingsCommand, UpdateDeploymentDisplaySettingsCommand,
    UpdateDeploymentEmailTemplateCommand, UpdateDeploymentJwtTemplateCommand,
    UpdateDeploymentRestrictionsCommand,
};
use dto::{
    json::{
        DeploymentAuthSettingsUpdates, DeploymentDisplaySettingsUpdates,
        DeploymentRestrictionsUpdates, NewDeploymentJwtTemplate, PartialDeploymentJwtTemplate,
    },
    params::deployment::DeploymentNameParams,
};
use models::{DeploymentJwtTemplate, DeploymentWithSettings, EmailTemplate};
use queries::{
    GetDeploymentEmailTemplateQuery, Query,
    deployment::{GetDeploymentJwtTemplatesQuery, GetDeploymentWithSettingsQuery},
};

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

// Path parameter struct for email template routes
#[derive(Deserialize)]
pub struct EmailTemplateParams {
    pub deployment_id: i64,
    pub template_name: DeploymentNameParams,
}

pub async fn get_deployment_with_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<DeploymentWithSettings> {
    GetDeploymentWithSettingsQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_display_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentDisplaySettingsUpdates>,
) -> ApiResult<()> {
    UpdateDeploymentDisplaySettingsCommand::new(deployment_id, updates)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_auth_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentAuthSettingsUpdates>,
) -> ApiResult<()> {
    UpdateDeploymentAuthSettingsCommand::new(deployment_id, updates)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_restrictions(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentRestrictionsUpdates>,
) -> ApiResult<()> {
    UpdateDeploymentRestrictionsCommand::new(deployment_id, updates)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_deployment_jwt_templates(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<DeploymentJwtTemplate>> {
    let templates = GetDeploymentJwtTemplatesQuery::new(deployment_id)
        .execute(&app_state)
        .await?;

    Ok(PaginatedResponse {
        data: templates,
        has_more: false,
        limit: None,
        offset: None,
    }
    .into())
}

pub async fn create_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(template): Json<NewDeploymentJwtTemplate>,
) -> ApiResult<DeploymentJwtTemplate> {
    CreateDeploymentJwtTemplateCommand::new(deployment_id, template)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(id): Path<i64>,
    Json(updates): Json<PartialDeploymentJwtTemplate>,
) -> ApiResult<DeploymentJwtTemplate> {
    UpdateDeploymentJwtTemplateCommand::new(deployment_id, id, updates)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(id): Path<i64>,
) -> ApiResult<()> {
    DeleteDeploymentJwtTemplateCommand::new(deployment_id, id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_deployment_email_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<EmailTemplateParams>,
) -> ApiResult<EmailTemplate> {
    GetDeploymentEmailTemplateQuery::new(deployment_id, params.template_name)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_email_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<EmailTemplateParams>,
    Json(template): Json<EmailTemplate>,
) -> ApiResult<EmailTemplate> {
    UpdateDeploymentEmailTemplateCommand::new(deployment_id, params.template_name, template)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
