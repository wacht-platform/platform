use crate::{
    api::multipart::{MultipartField, MultipartPayload},
    application::response::{ApiErrorResponse, ApiResult, PaginatedResponse},
    middleware::RequireDeployment,
};
use common::state::AppState;

use commands::{
    Command, CreateUserCommand, DeleteUserCommand, GenerateImpersonationTokenCommand,
    UpdateUserCommand, UpdateUserPasswordCommand, UpdateUserProfileImageCommand, UploadToCdnCommand,
};
use dto::{
    json::{CreateUserRequest, UpdatePasswordRequest, UpdateUserRequest},
    query::ActiveUserListQueryParams,
};
use models::{UserDetails, UserWithIdentifiers};
use queries::{DeploymentActiveUserListQuery, GetUserDetailsQuery, Query};

use axum::{
    Json,
    extract::{Multipart, Path, Query as QueryParams, State},
    http::StatusCode,
};

use super::types::UserParams;
use super::validators::{validate_create_user_request, validate_update_user_request};

fn parse_image_upload(field: &MultipartField) -> Result<Option<(Vec<u8>, String)>, ApiErrorResponse> {
    let Some(file_extension) = field.image_extension()? else {
        return Ok(None);
    };

    if field.bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some((field.bytes.clone(), file_extension.to_string())))
}

pub async fn get_active_user_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<ActiveUserListQueryParams>,
) -> ApiResult<PaginatedResponse<UserWithIdentifiers>> {
    let limit = params.limit.unwrap_or(10) as i32;

    let users = DeploymentActiveUserListQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(params.offset.unwrap_or(0))
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute(&app_state)
        .await
        .unwrap();

    let has_more = users.len() > limit as usize;
    let users = if has_more {
        users[..limit as usize].to_vec()
    } else {
        users
    };

    Ok(PaginatedResponse {
        data: users,
        has_more,
        limit: Some(limit),
        offset: Some(params.offset.unwrap_or(0) as i32),
    }
    .into())
}

pub async fn get_user_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<UserDetails> {
    GetUserDetailsQuery::new(deployment_id, params.user_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

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
            "first_name" => {
                request.first_name = field.text()?;
            }
            "last_name" => {
                request.last_name = field.text()?;
            }
            "email_address" => {
                let email = field.text_trimmed()?;
                if !email.is_empty() {
                    request.email_address = Some(email);
                }
            }
            "phone_number" => {
                let phone = field.text_trimmed()?;
                if !phone.is_empty() {
                    request.phone_number = Some(phone);
                }
            }
            "username" => {
                let username = field.text_trimmed()?;
                if !username.is_empty() {
                    request.username = Some(username);
                }
            }
            "password" => {
                let password = field.text_trimmed()?;
                if !password.is_empty() {
                    request.password = Some(password);
                }
            }
            "skip_password_check" => {
                let value = field.text()?;
                request.skip_password_check = value == "true";
            }
            "profile_image" => {
                if let Some(image) = parse_image_upload(field)? {
                    profile_image_data = Some(image);
                }
            }
            _ => {
                // Skip unknown fields
            }
        }
    }

    // Validate fields based on deployment settings
    validate_create_user_request(&app_state, deployment_id, &request).await?;

    // Create the user first
    let user = CreateUserCommand::new(deployment_id, request)
        .execute(&app_state)
        .await?;

    // If there's a profile image, upload it and update the user
    if let Some((image_buffer, file_extension)) = profile_image_data {
        let file_path = format!(
            "deployments/{}/users/{}/profile.{}",
            deployment_id, user.id, file_extension
        );

        let url = UploadToCdnCommand::new(file_path, image_buffer)
            .execute(&app_state)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        UpdateUserProfileImageCommand::new(deployment_id, user.id, url)
            .execute(&app_state)
            .await?;
    }

    Ok(user.into())
}

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
                let metadata_str = field.text()?;
                if !metadata_str.is_empty() {
                    if let Ok(metadata) = serde_json::from_str(&metadata_str) {
                        request.public_metadata = Some(metadata);
                    }
                }
            }
            "private_metadata" => {
                let metadata_str = field.text()?;
                if !metadata_str.is_empty() {
                    if let Ok(metadata) = serde_json::from_str(&metadata_str) {
                        request.private_metadata = Some(metadata);
                    }
                }
            }
            "disabled" => {
                let disabled_str = field.text()?;
                if let Ok(disabled) = disabled_str.parse::<bool>() {
                    request.disabled = Some(disabled);
                }
            }
            "remove_profile_image" => {
                let value = field.text()?;
                remove_profile_image = value == "true";
            }
            "profile_image" => {
                if let Some(image) = parse_image_upload(field)? {
                    profile_image_data = Some(image);
                }
            }
            _ => {
                // Skip unknown fields
            }
        }
    }

    // Validate fields based on deployment settings
    validate_update_user_request(&app_state, deployment_id, &request).await?;

    // Update user fields
    let user_details = UpdateUserCommand::new(deployment_id, params.user_id, request)
        .execute(&app_state)
        .await?;

    // Handle profile image removal
    if remove_profile_image {
        UpdateUserProfileImageCommand::new(deployment_id, params.user_id, String::new())
            .execute(&app_state)
            .await?;

        // Fetch updated user details to return
        let updated_user_details = GetUserDetailsQuery::new(deployment_id, params.user_id)
            .execute(&app_state)
            .await?;

        return Ok(updated_user_details.into());
    }

    // If there's a profile image, upload it and update the user
    if let Some((image_buffer, file_extension)) = profile_image_data {
        let file_path = format!(
            "deployments/{}/users/{}/profile.{}",
            deployment_id, params.user_id, file_extension
        );

        let url = UploadToCdnCommand::new(file_path, image_buffer)
            .execute(&app_state)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        UpdateUserProfileImageCommand::new(deployment_id, params.user_id, url)
            .execute(&app_state)
            .await?;

        // Fetch updated user details to return the new profile picture URL
        let updated_user_details = GetUserDetailsQuery::new(deployment_id, params.user_id)
            .execute(&app_state)
            .await?;

        return Ok(updated_user_details.into());
    }

    Ok(user_details.into())
}


pub async fn update_user_password(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(request): Json<UpdatePasswordRequest>,
) -> ApiResult<()> {
    UpdateUserPasswordCommand::new(
        deployment_id,
        params.user_id,
        request.new_password,
        request.skip_password_check,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn delete_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<()> {
    DeleteUserCommand::new(deployment_id, params.user_id)
        .execute(&app_state)
        .await
        .unwrap();

    Ok(().into())
}

pub async fn impersonate_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<commands::GenerateImpersonationTokenResponse> {
    GenerateImpersonationTokenCommand::new(deployment_id, params.user_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
