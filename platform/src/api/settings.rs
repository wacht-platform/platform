use crate::{
    application::response::{ApiResult, PaginatedResponse},
    middleware::RequireDeployment,
};
use common::state::AppState;

use commands::{
    Command, CreateDeploymentJwtTemplateCommand, DeleteDeploymentJwtTemplateCommand,
    RemoveDeploymentSmtpConfigCommand,
    UpdateDeploymentAuthSettingsCommand, UpdateDeploymentDisplaySettingsCommand,
    UpdateDeploymentEmailTemplateCommand, UpdateDeploymentJwtTemplateCommand,
    UpdateDeploymentRestrictionsCommand, UpdateDeploymentSmtpConfigCommand,
    VerifySmtpConnectionCommand,
};
use dto::{
    json::{
        DeploymentAuthSettingsUpdates, DeploymentDisplaySettingsUpdates,
        DeploymentRestrictionsUpdates, NewDeploymentJwtTemplate, PartialDeploymentJwtTemplate,
        SmtpConfigRequest, SmtpConfigResponse,
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
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct EmailTemplateParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub template_name: DeploymentNameParams,
}

#[derive(Deserialize)]
pub struct JWTTemplateParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub id: i64,
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
    Path(params): Path<JWTTemplateParams>,
    Json(updates): Json<PartialDeploymentJwtTemplate>,
) -> ApiResult<DeploymentJwtTemplate> {
    UpdateDeploymentJwtTemplateCommand::new(deployment_id, params.id, updates)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<JWTTemplateParams>,
) -> ApiResult<()> {
    DeleteDeploymentJwtTemplateCommand::new(deployment_id, params.id)
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

pub async fn verify_smtp_connection(
    State(app_state): State<AppState>,
    RequireDeployment(_deployment_id): RequireDeployment,
    Json(config): Json<SmtpConfigRequest>,
) -> ApiResult<()> {
    VerifySmtpConnectionCommand::new(
        config.host,
        config.port,
        config.username,
        config.password,
        config.from_email,
        config.use_tls,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn update_smtp_config(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(config): Json<SmtpConfigRequest>,
) -> ApiResult<SmtpConfigResponse> {
    VerifySmtpConnectionCommand::new(
        config.host.clone(),
        config.port,
        config.username.clone(),
        config.password.clone(),
        config.from_email.clone(),
        config.use_tls,
    )
    .execute(&app_state)
    .await?;

    let result = UpdateDeploymentSmtpConfigCommand::new(
        deployment_id,
        config.host,
        config.port,
        config.username,
        config.password,
        config.from_email,
        config.use_tls,
    )
    .execute(&app_state)
    .await?;

    Ok(SmtpConfigResponse {
        host: result.host,
        port: result.port,
        username: result.username,
        from_email: result.from_email,
        use_tls: result.use_tls,
        verified: result.verified,
    }
    .into())
}

pub async fn remove_smtp_config(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<()> {
    RemoveDeploymentSmtpConfigCommand::new(deployment_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
