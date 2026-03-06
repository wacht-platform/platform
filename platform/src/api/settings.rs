use crate::{
    application::{
        response::{ApiResult, PaginatedResponse},
        settings as settings_use_cases,
    },
    middleware::RequireDeployment,
};
use common::state::AppState;

use dto::{
    json::{
        DeploymentAuthSettingsUpdates, DeploymentDisplaySettingsUpdates,
        DeploymentRestrictionsUpdates, NewDeploymentJwtTemplate, PartialDeploymentJwtTemplate,
        SmtpConfigRequest, SmtpConfigResponse, SmtpVerifyResponse,
    },
    params::deployment::DeploymentNameParams,
};
use models::{DeploymentJwtTemplate, DeploymentWithSettings, EmailTemplate};

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
    let deployment =
        settings_use_cases::get_deployment_with_settings(&app_state, deployment_id).await?;
    Ok(deployment.into())
}

pub async fn update_deployment_display_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentDisplaySettingsUpdates>,
) -> ApiResult<()> {
    settings_use_cases::update_deployment_display_settings(&app_state, deployment_id, updates)
        .await?;
    Ok(().into())
}

pub async fn update_deployment_auth_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentAuthSettingsUpdates>,
) -> ApiResult<()> {
    settings_use_cases::update_deployment_auth_settings(&app_state, deployment_id, updates).await?;
    Ok(().into())
}

pub async fn update_deployment_restrictions(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentRestrictionsUpdates>,
) -> ApiResult<()> {
    settings_use_cases::update_deployment_restrictions(&app_state, deployment_id, updates).await?;
    Ok(().into())
}

pub async fn get_deployment_jwt_templates(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<DeploymentJwtTemplate>> {
    let templates =
        settings_use_cases::get_deployment_jwt_templates(&app_state, deployment_id).await?;

    Ok(PaginatedResponse::from(templates).into())
}

pub async fn create_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(template): Json<NewDeploymentJwtTemplate>,
) -> ApiResult<DeploymentJwtTemplate> {
    let jwt_template =
        settings_use_cases::create_deployment_jwt_template(&app_state, deployment_id, template)
            .await?;
    Ok(jwt_template.into())
}

pub async fn update_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<JWTTemplateParams>,
    Json(updates): Json<PartialDeploymentJwtTemplate>,
) -> ApiResult<DeploymentJwtTemplate> {
    let jwt_template = settings_use_cases::update_deployment_jwt_template(
        &app_state,
        deployment_id,
        params.id,
        updates,
    )
    .await?;
    Ok(jwt_template.into())
}

pub async fn delete_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<JWTTemplateParams>,
) -> ApiResult<()> {
    settings_use_cases::delete_deployment_jwt_template(&app_state, deployment_id, params.id)
        .await?;
    Ok(().into())
}

pub async fn get_deployment_email_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<EmailTemplateParams>,
) -> ApiResult<EmailTemplate> {
    let template = settings_use_cases::get_deployment_email_template(
        &app_state,
        deployment_id,
        params.template_name,
    )
    .await?;
    Ok(template.into())
}

pub async fn update_deployment_email_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<EmailTemplateParams>,
    Json(template): Json<EmailTemplate>,
) -> ApiResult<EmailTemplate> {
    let updated = settings_use_cases::update_deployment_email_template(
        &app_state,
        deployment_id,
        params.template_name,
        template,
    )
    .await?;
    Ok(updated.into())
}

pub async fn verify_smtp_connection(
    State(app_state): State<AppState>,
    RequireDeployment(_deployment_id): RequireDeployment,
    Json(config): Json<SmtpConfigRequest>,
) -> ApiResult<SmtpVerifyResponse> {
    let response = settings_use_cases::verify_smtp_connection(&app_state, config).await?;
    Ok(response.into())
}

pub async fn update_smtp_config(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(config): Json<SmtpConfigRequest>,
) -> ApiResult<SmtpConfigResponse> {
    let response =
        settings_use_cases::update_smtp_config(&app_state, deployment_id, config).await?;
    Ok(response.into())
}

pub async fn remove_smtp_config(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<()> {
    settings_use_cases::remove_smtp_config(&app_state, deployment_id).await?;
    Ok(().into())
}
