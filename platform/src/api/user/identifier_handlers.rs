use crate::{
    application::{response::ApiResult, user_identifier as user_identifier_use_cases},
    middleware::RequireDeployment,
};
use common::state::AppState;

use dto::json::{AddEmailRequest, AddPhoneRequest, UpdateEmailRequest, UpdatePhoneRequest};
use models::{UserEmailAddress, UserPhoneNumber};

use axum::{
    Json,
    extract::{Path, State},
};

use super::types::{UserEmailParams, UserParams, UserPhoneParams, UserSocialParams};

pub async fn add_user_email(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(request): Json<AddEmailRequest>,
) -> ApiResult<UserEmailAddress> {
    let email = user_identifier_use_cases::add_user_email(
        &app_state,
        deployment_id,
        params.user_id,
        request,
    )
    .await?;
    Ok(email.into())
}

pub async fn update_user_email(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserEmailParams>,
    Json(request): Json<UpdateEmailRequest>,
) -> ApiResult<UserEmailAddress> {
    let email = user_identifier_use_cases::update_user_email(
        &app_state,
        deployment_id,
        params.user_id,
        params.email_id,
        request,
    )
    .await?;
    Ok(email.into())
}

pub async fn delete_user_email(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserEmailParams>,
) -> ApiResult<()> {
    user_identifier_use_cases::delete_user_email(&app_state, params.user_id, params.email_id)
        .await?;
    Ok(().into())
}

pub async fn add_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(request): Json<AddPhoneRequest>,
) -> ApiResult<UserPhoneNumber> {
    let phone = user_identifier_use_cases::add_user_phone(
        &app_state,
        deployment_id,
        params.user_id,
        request,
    )
    .await?;
    Ok(phone.into())
}

pub async fn update_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserPhoneParams>,
    Json(request): Json<UpdatePhoneRequest>,
) -> ApiResult<UserPhoneNumber> {
    let phone = user_identifier_use_cases::update_user_phone(
        &app_state,
        params.user_id,
        params.phone_id,
        request,
    )
    .await?;
    Ok(phone.into())
}

pub async fn delete_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserPhoneParams>,
) -> ApiResult<()> {
    user_identifier_use_cases::delete_user_phone(&app_state, params.user_id, params.phone_id)
        .await?;
    Ok(().into())
}

pub async fn delete_user_social_connection(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserSocialParams>,
) -> ApiResult<()> {
    user_identifier_use_cases::delete_user_social_connection(
        &app_state,
        params.user_id,
        params.connection_id,
    )
    .await?;
    Ok(().into())
}
