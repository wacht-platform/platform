use crate::{application::response::ApiResult, middleware::RequireDeployment};
use common::state::AppState;

use commands::{
    AddUserEmailCommand, AddUserPhoneCommand, Command, DeleteUserEmailCommand,
    DeleteUserPhoneCommand, DeleteUserSocialConnectionCommand, UpdateUserEmailCommand,
    UpdateUserPhoneCommand,
};
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
    let email = AddUserEmailCommand::new(deployment_id, params.user_id, request)
        .execute(&app_state)
        .await?;
    Ok(email.into())
}

pub async fn update_user_email(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserEmailParams>,
    Json(request): Json<UpdateEmailRequest>,
) -> ApiResult<UserEmailAddress> {
    let email =
        UpdateUserEmailCommand::new(deployment_id, params.user_id, params.email_id, request)
            .execute(&app_state)
            .await?;
    Ok(email.into())
}

pub async fn delete_user_email(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserEmailParams>,
) -> ApiResult<()> {
    DeleteUserEmailCommand::new(params.user_id, params.email_id)
        .execute(&app_state)
        .await?;

    Ok(().into())
}

pub async fn add_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(request): Json<AddPhoneRequest>,
) -> ApiResult<UserPhoneNumber> {
    let phone = AddUserPhoneCommand::new(deployment_id, params.user_id, request)
        .execute(&app_state)
        .await?;
    Ok(phone.into())
}

pub async fn update_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserPhoneParams>,
    Json(request): Json<UpdatePhoneRequest>,
) -> ApiResult<UserPhoneNumber> {
    let phone = UpdateUserPhoneCommand::new(params.user_id, params.phone_id, request)
        .execute(&app_state)
        .await?;
    Ok(phone.into())
}

pub async fn delete_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserPhoneParams>,
) -> ApiResult<()> {
    DeleteUserPhoneCommand::new(params.user_id, params.phone_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn delete_user_social_connection(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserSocialParams>,
) -> ApiResult<()> {
    DeleteUserSocialConnectionCommand::new(params.user_id, params.connection_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}
