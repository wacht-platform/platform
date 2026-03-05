use crate::{
    api::multipart::{MultipartField, MultipartPayload},
    api::pagination::paginate_results,
    application::response::{ApiErrorResponse, ApiResult, PaginatedResponse},
    middleware::RequireDeployment,
};
use common::state::AppState;

use commands::{
    Command, CreateUserCommand, DeleteUserCommand, GenerateImpersonationTokenCommand,
    UpdateUserCommand, UpdateUserPasswordCommand, UpdateUserProfileImageCommand,
    UploadToCdnCommand,
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

async fn refresh_user_details(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> ApiResult<UserDetails> {
    let user_details = GetUserDetailsQuery::new(deployment_id, user_id)
        .execute(app_state)
        .await?;
    Ok(user_details.into())
}

async fn upload_user_profile_image(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    image_buffer: Vec<u8>,
    file_extension: String,
) -> Result<String, ApiErrorResponse> {
    let file_path = format!(
        "deployments/{}/users/{}/profile.{}",
        deployment_id, user_id, file_extension
    );

    UploadToCdnCommand::new(file_path, image_buffer)
        .execute(app_state)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into())
}

pub async fn get_active_user_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<ActiveUserListQueryParams>,
) -> ApiResult<PaginatedResponse<UserWithIdentifiers>> {
    let limit = params.limit.unwrap_or(10) as i32;
    let offset = params.offset.unwrap_or(0);

    let users = DeploymentActiveUserListQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(offset)
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute(&app_state)
        .await?;

    Ok(paginate_results(users, limit, Some(offset)).into())
}

pub async fn get_user_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<UserDetails> {
    let user_details = GetUserDetailsQuery::new(deployment_id, params.user_id)
        .execute(&app_state)
        .await?;
    Ok(user_details.into())
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
                request.email_address = field.optional_text_trimmed()?;
            }
            "phone_number" => {
                request.phone_number = field.optional_text_trimmed()?;
            }
            "username" => {
                request.username = field.optional_text_trimmed()?;
            }
            "password" => {
                request.password = field.optional_text_trimmed()?;
            }
            "skip_password_check" => {
                request.skip_password_check = field.bool_true()?;
            }
            "profile_image" => {
                if let Some(image) = field.image_upload()? {
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
        let url = upload_user_profile_image(
            &app_state,
            deployment_id,
            user.id,
            image_buffer,
            file_extension,
        )
        .await?;

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
            "remove_profile_image" => {
                remove_profile_image = field.bool_true()?;
            }
            "profile_image" => {
                if let Some(image) = field.image_upload()? {
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

        return refresh_user_details(&app_state, deployment_id, params.user_id).await;
    }

    // If there's a profile image, upload it and update the user
    if let Some((image_buffer, file_extension)) = profile_image_data {
        let url = upload_user_profile_image(
            &app_state,
            deployment_id,
            params.user_id,
            image_buffer,
            file_extension,
        )
        .await?;

        UpdateUserProfileImageCommand::new(deployment_id, params.user_id, url)
            .execute(&app_state)
            .await?;

        return refresh_user_details(&app_state, deployment_id, params.user_id).await;
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
    .await?;
    Ok(().into())
}

pub async fn delete_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<()> {
    DeleteUserCommand::new(deployment_id, params.user_id)
        .execute(&app_state)
        .await?;

    Ok(().into())
}

pub async fn impersonate_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<commands::GenerateImpersonationTokenResponse> {
    let response = GenerateImpersonationTokenCommand::new(deployment_id, params.user_id)
        .execute(&app_state)
        .await?;
    Ok(response.into())
}
