use crate::{
    application::{
        response::{ApiResult, PaginatedResponse},
        settings as deployment_settings,
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
        deployment_settings::get_deployment_with_settings(&app_state, deployment_id).await?;
    Ok(deployment.into())
}

pub async fn update_deployment_display_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentDisplaySettingsUpdates>,
) -> ApiResult<()> {
    deployment_settings::update_deployment_display_settings(&app_state, deployment_id, updates)
        .await?;
    Ok(().into())
}

pub async fn update_deployment_auth_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentAuthSettingsUpdates>,
) -> ApiResult<()> {
    deployment_settings::update_deployment_auth_settings(&app_state, deployment_id, updates)
        .await?;
    Ok(().into())
}

pub async fn update_deployment_restrictions(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<DeploymentRestrictionsUpdates>,
) -> ApiResult<()> {
    deployment_settings::update_deployment_restrictions(&app_state, deployment_id, updates).await?;
    Ok(().into())
}

pub async fn get_deployment_jwt_templates(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<DeploymentJwtTemplate>> {
    let templates =
        deployment_settings::get_deployment_jwt_templates(&app_state, deployment_id).await?;

    Ok(PaginatedResponse::from(templates).into())
}

pub async fn create_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(template): Json<NewDeploymentJwtTemplate>,
) -> ApiResult<DeploymentJwtTemplate> {
    let jwt_template = deployment_settings::create_deployment_jwt_template(
        &app_state,
        deployment_id,
        template,
    )
    .await?;
    Ok(jwt_template.into())
}

pub async fn update_deployment_jwt_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<JWTTemplateParams>,
    Json(updates): Json<PartialDeploymentJwtTemplate>,
) -> ApiResult<DeploymentJwtTemplate> {
    let jwt_template = deployment_settings::update_deployment_jwt_template(
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
    deployment_settings::delete_deployment_jwt_template(&app_state, deployment_id, params.id)
        .await?;
    Ok(().into())
}

pub async fn get_deployment_email_template(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<EmailTemplateParams>,
) -> ApiResult<EmailTemplate> {
    let template = deployment_settings::get_deployment_email_template(
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
    let updated = deployment_settings::update_deployment_email_template(
        &app_state,
        deployment_id,
        params.template_name,
        template,
    )
    .await?;
    Ok(updated.into())
}

pub async fn verify_smtp_connection(
    RequireDeployment(_deployment_id): RequireDeployment,
    Json(config): Json<SmtpConfigRequest>,
) -> ApiResult<SmtpVerifyResponse> {
    let response = deployment_settings::verify_smtp_connection(config).await?;
    Ok(response.into())
}

pub async fn update_smtp_config(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(config): Json<SmtpConfigRequest>,
) -> ApiResult<SmtpConfigResponse> {
    let response =
        deployment_settings::update_smtp_config(&app_state, deployment_id, config).await?;
    Ok(response.into())
}

pub async fn remove_smtp_config(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<()> {
    deployment_settings::remove_smtp_config(&app_state, deployment_id).await?;
    Ok(().into())
}
