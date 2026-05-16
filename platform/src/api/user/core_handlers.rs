use crate::{
    api::multipart::{MultipartField, MultipartPayload},
    application::{
        response::{ApiErrorResponse, ApiResult, PaginatedResponse},
        user_core as user_core_app,
    },
    middleware::RequireDeployment,
};
use common::state::AppState;

use dto::{
    json::{CreateUserRequest, UpdatePasswordRequest, UpdateUserRequest},
    query::ActiveUserListQueryParams,
};
use models::{UserDetails, UserWithIdentifiers};

use axum::{
    Json,
    extract::{Multipart, Path, Query as QueryParams, State},
};

use super::types::UserParams;
use super::validators::{validate_create_user_request, validate_update_user_request};

fn parse_json_value_field(
    field: &MultipartField,
) -> Result<Option<serde_json::Value>, ApiErrorResponse> {
    let metadata_str = field.text()?;
    if metadata_str.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str(&metadata_str) {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

pub async fn get_active_user_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<ActiveUserListQueryParams>,
) -> ApiResult<PaginatedResponse<UserWithIdentifiers>> {
    let users = user_core_app::get_active_user_list(&app_state, deployment_id, params).await?;
    Ok(users.into())
}

pub async fn get_user_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<UserDetails> {
    let user_details =
        user_core_app::get_user_details(&app_state, deployment_id, params.user_id).await?;
    Ok(user_details.into())
}

/// Multipart form fields:
/// - first_name: string required
/// - last_name: string required
/// - email_address: string optional
/// - phone_number: string optional
/// - username: string optional
/// - password: string optional
/// - skip_password_check: flag optional
/// - profile_image: file optional
pub async fn create_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    multipart: Multipart,
) -> ApiResult<UserWithIdentifiers> {
    let mut request = CreateUserRequest {
        first_name: String::new(),
        last_name: String::new(),
        email_address: None,
        phone_number: None,
        username: None,
        password: None,
        skip_password_check: false,
    };

    let mut profile_image_data: Option<(Vec<u8>, String)> = None;
    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        match field.name.as_str() {
            "first_name" => request.first_name = field.text()?,
            "last_name" => request.last_name = field.text()?,
            "email_address" => request.email_address = field.optional_text_trimmed()?,
            "phone_number" => request.phone_number = field.optional_text_trimmed()?,
            "username" => request.username = field.optional_text_trimmed()?,
            "password" => request.password = field.optional_text_trimmed()?,
            "skip_password_check" => request.skip_password_check = field.bool_true()?,
            "profile_image" => {
                if let Some(image) = field.image_upload()? {
                    profile_image_data = Some(image);
                }
            }
            _ => {}
        }
    }

    validate_create_user_request(&app_state, deployment_id, &request).await?;

    let user =
        user_core_app::create_user(&app_state, deployment_id, request, profile_image_data).await?;
    Ok(user.into())
}

/// Multipart form fields:
/// - first_name: string optional
/// - last_name: string optional
/// - username: string optional
/// - public_metadata: json optional
/// - private_metadata: json optional
/// - disabled: flag optional
/// - second_factor_policy: string optional — one of `none`, `optional`, `enforced`
/// - remove_profile_image: flag optional
/// - profile_image: file optional
pub async fn update_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    multipart: Multipart,
) -> ApiResult<UserDetails> {
    let mut request = UpdateUserRequest {
        first_name: None,
        last_name: None,
        username: None,
        public_metadata: None,
        private_metadata: None,
        disabled: None,
        second_factor_policy: None,
    };

    let mut profile_image_data: Option<(Vec<u8>, String)> = None;
    let mut remove_profile_image = false;

    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        match field.name.as_str() {
            "first_name" => {
                let first_name = field.text()?;
                if !first_name.is_empty() {
                    request.first_name = Some(first_name);
                }
            }
            "last_name" => {
                let last_name = field.text()?;
                if !last_name.is_empty() {
                    request.last_name = Some(last_name);
                }
            }
            "username" => {
                let username = field.text()?;
                if !username.is_empty() {
                    request.username = Some(username);
                }
            }
            "public_metadata" => {
                if let Some(metadata) = parse_json_value_field(field)? {
                    request.public_metadata = Some(metadata);
                }
            }
            "private_metadata" => {
                if let Some(metadata) = parse_json_value_field(field)? {
                    request.private_metadata = Some(metadata);
                }
            }
            "disabled" => {
                let disabled_str = field.text()?;
                if let Ok(disabled) = disabled_str.parse::<bool>() {
                    request.disabled = Some(disabled);
                }
            }
            "second_factor_policy" => {
                let policy_str = field.text()?;
                if !policy_str.is_empty() {
                    request.second_factor_policy = Some(policy_str.parse().map_err(|_| {
                        common::error::AppError::BadRequest(
                            "second_factor_policy must be one of: none, optional, enforced"
                                .to_string(),
                        )
                    })?);
                }
            }
            "remove_profile_image" => remove_profile_image = field.bool_true()?,
            "profile_image" => {
                if let Some(image) = field.image_upload()? {
                    profile_image_data = Some(image);
                }
            }
            _ => {}
        }
    }

    validate_update_user_request(&app_state, deployment_id, &request).await?;

    let user_details = user_core_app::update_user(
        &app_state,
        deployment_id,
        params.user_id,
        request,
        profile_image_data,
        remove_profile_image,
    )
    .await?;

    Ok(user_details.into())
}

pub async fn update_user_password(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(request): Json<UpdatePasswordRequest>,
) -> ApiResult<()> {
    user_core_app::update_user_password(&app_state, deployment_id, params.user_id, request).await?;
    Ok(().into())
}

pub async fn delete_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<()> {
    user_core_app::delete_user(&app_state, deployment_id, params.user_id).await?;
    Ok(().into())
}

pub async fn remove_user_password(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<()> {
    user_core_app::remove_user_password(&app_state, deployment_id, params.user_id).await?;
    Ok(().into())
}

pub async fn impersonate_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<commands::GenerateImpersonationTokenResponse> {
    let response =
        user_core_app::impersonate_user(&app_state, deployment_id, params.user_id).await?;
    Ok(response.into())
}
