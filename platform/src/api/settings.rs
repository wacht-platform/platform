use crate::{
    application::{
        HttpState,
        response::{ApiResult, PaginatedResponse},
    },
    middleware::RequireDeployment,
    core::{
        commands::{
            Command, CreateDeploymentJwtTemplateCommand, DeleteDeploymentJwtTemplateCommand,
            UpdateDeploymentAuthSettingsCommand, UpdateDeploymentDisplaySettingsCommand,
            UpdateDeploymentEmailTemplateCommand, UpdateDeploymentJwtTemplateCommand,
            UpdateDeploymentRestrictionsCommand,
        },
        dto::{
            json::{
                DeploymentAuthSettingsUpdates, DeploymentDisplaySettingsUpdates,
                DeploymentRestrictionsUpdates, NewDeploymentJwtTemplate,
                PartialDeploymentJwtTemplate,
            },
            params::deployment::DeploymentNameParams,
        },
        models::{DeploymentJwtTemplate, DeploymentWithSettings, EmailTemplate},
        queries::{
            GetDeploymentEmailTemplateQuery, Query,
            deployment::{GetDeploymentJwtTemplatesQuery, GetDeploymentWithSettingsQuery},
        },
    },
};
use axum::{
    Json,
    extract::{Path, State},
};

pub async fn get_deployment_with_settings(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<DeploymentWithSettings> {
    GetDeploymentWithSettingsQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_authetication_settings(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(settings): Json<DeploymentAuthSettingsUpdates>,
) -> ApiResult<()> {
    UpdateDeploymentAuthSettingsCommand::new(deployment_id, settings)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_restrictions(
    State(app_state): State<HttpState>,
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
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<DeploymentJwtTemplate>> {
    GetDeploymentJwtTemplatesQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .map(PaginatedResponse::from)
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn create_deployment_jwt_template(
    State(app_state): State<HttpState>,
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
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(id): Path<i64>,
    Json(template): Json<PartialDeploymentJwtTemplate>,
) -> ApiResult<DeploymentJwtTemplate> {
    UpdateDeploymentJwtTemplateCommand::new(deployment_id, id, template)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_deployment_jwt_template(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(id): Path<i64>,
) -> ApiResult<()> {
    DeleteDeploymentJwtTemplateCommand::new(deployment_id, id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_ui_settings(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(settings): Json<DeploymentDisplaySettingsUpdates>,
) -> ApiResult<()> {
    UpdateDeploymentDisplaySettingsCommand::new(deployment_id, settings)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_email_template(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(template_name): Path<DeploymentNameParams>,
) -> ApiResult<EmailTemplate> {
    GetDeploymentEmailTemplateQuery::new(deployment_id, template_name)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_email_template(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(template_name): Path<DeploymentNameParams>,
    Json(template): Json<EmailTemplate>,
) -> ApiResult<EmailTemplate> {
    UpdateDeploymentEmailTemplateCommand::new(deployment_id, template_name, template)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
