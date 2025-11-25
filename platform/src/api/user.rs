use crate::{
    application::response::{ApiResult, PaginatedResponse},
    middleware::RequireDeployment,
};
use common::state::AppState;

use commands::{
    AddUserEmailCommand, AddUserPhoneCommand, ApproveWaitlistUserCommand, Command,
    CreateUserCommand, DeleteUserCommand, DeleteUserEmailCommand, DeleteUserPhoneCommand,
    DeleteUserSocialConnectionCommand, GenerateImpersonationTokenCommand, InviteUserCommand,
    UpdateUserCommand, UpdateUserEmailCommand, UpdateUserPasswordCommand, UpdateUserPhoneCommand,
    UpdateUserProfileImageCommand, UploadToCdnCommand,
};
use dto::{
    json::{
        AddEmailRequest, AddPhoneRequest, CreateUserRequest, InviteUserRequest, UpdateEmailRequest,
        UpdatePhoneRequest, UpdateUserRequest,
    },
    query::{ActiveUserListQueryParams, InvitationsWaitlistQueryParams},
};
use models::{
    DeploymentInvitation, DeploymentWaitlistUser, UserDetails, UserEmailAddress, UserPhoneNumber,
    UserWithIdentifiers,
};
use queries::{
    DeploymentActiveUserListQuery, DeploymentInvitationQuery, DeploymentWaitlistQuery,
    GetDeploymentAuthSettingsQuery, GetUserDetailsQuery, Query,
};

use axum::{
    Json,
    extract::{Multipart, Path, Query as QueryParams, State},
    http::StatusCode,
};
use serde::Deserialize;
use std::collections::HashMap;

// Path parameter structs for nested routes
#[derive(Deserialize)]
pub struct UserParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub user_id: i64,
}

#[derive(Deserialize)]
pub struct UserEmailParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub user_id: i64,
    pub email_id: i64,
}

#[derive(Deserialize)]
pub struct UserPhoneParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub user_id: i64,
    pub phone_id: i64,
}

#[derive(Deserialize)]
pub struct UserSocialParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub user_id: i64,
    pub connection_id: i64,
}

#[derive(Deserialize)]
pub struct WaitlistUserParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub waitlist_user_id: i64,
}

async fn validate_create_user_request(
    app_state: &AppState,
    deployment_id: i64,
    request: &CreateUserRequest,
) -> Result<(), (StatusCode, String)> {
    let auth_settings = GetDeploymentAuthSettingsQuery::new(deployment_id)
        .execute(app_state)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get deployment auth settings: {}", e),
            )
        })?;

    if auth_settings.first_name.enabled
        && auth_settings.first_name.required.unwrap_or(true)
        && request.first_name.trim().is_empty()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "First name is required".to_string(),
        ));
    }

    if auth_settings.last_name.enabled
        && auth_settings.last_name.required.unwrap_or(true)
        && request.last_name.trim().is_empty()
    {
        return Err((StatusCode::BAD_REQUEST, "Last name is required".to_string()));
    }

    if auth_settings.email_address.enabled
        && auth_settings.email_address.required
        && request.email_address.is_none()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Email address is required".to_string(),
        ));
    }

    if auth_settings.phone_number.enabled
        && auth_settings.phone_number.required
        && request.phone_number.is_none()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Phone number is required".to_string(),
        ));
    }

    if auth_settings.username.enabled
        && auth_settings.username.required
        && request.username.is_none()
    {
        return Err((StatusCode::BAD_REQUEST, "Username is required".to_string()));
    }

    if auth_settings.password.enabled {
        if let Some(password) = &request.password {
            if let Some(min_length) = auth_settings.password.min_length {
                if password.len() < min_length as usize {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("Password must be at least {} characters", min_length),
                    ));
                }
            }
        } else if auth_settings.password.min_length.is_some()
            && auth_settings.password.min_length.unwrap() > 0
        {
            return Err((StatusCode::BAD_REQUEST, "Password is required".to_string()));
        }
    }

    Ok(())
}

async fn validate_update_user_request(
    app_state: &AppState,
    deployment_id: i64,
    request: &UpdateUserRequest,
) -> Result<(), (StatusCode, String)> {
    let auth_settings = GetDeploymentAuthSettingsQuery::new(deployment_id)
        .execute(app_state)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get deployment auth settings: {}", e),
            )
        })?;

    if let Some(first_name) = &request.first_name {
        if auth_settings.first_name.enabled
            && auth_settings.first_name.required.unwrap_or(true)
            && first_name.trim().is_empty()
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "First name cannot be empty".to_string(),
            ));
        }
    }

    if let Some(last_name) = &request.last_name {
        if auth_settings.last_name.enabled
            && auth_settings.last_name.required.unwrap_or(true)
            && last_name.trim().is_empty()
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Last name cannot be empty".to_string(),
            ));
        }
    }

    if let Some(username) = &request.username {
        if auth_settings.username.enabled {
            if auth_settings.username.required && username.trim().is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Username cannot be empty".to_string(),
                ));
            }

            let username_len = username.trim().len();
            if let Some(min_length) = auth_settings.username.min_length {
                if username_len < min_length as usize {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("Username must be at least {} characters", min_length),
                    ));
                }
            }
            if let Some(max_length) = auth_settings.username.max_length {
                if username_len > max_length as usize {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("Username must be at most {} characters", max_length),
                    ));
                }
            }
        }
    }

    Ok(())
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

    Ok(PaginatedResponse::from(users).into())
}

pub async fn get_invited_user_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<InvitationsWaitlistQueryParams>,
) -> ApiResult<PaginatedResponse<DeploymentInvitation>> {
    let limit = params.limit.unwrap_or(10) as i32;

    let invitations = DeploymentInvitationQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(params.offset.unwrap_or(0))
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .execute(&app_state)
        .await
        .unwrap();

    let has_more = invitations.len() > limit as usize;
    let invitations = if has_more {
        invitations[..limit as usize].to_vec()
    } else {
        invitations
    };

    Ok(PaginatedResponse::from(invitations).into())
}

pub async fn get_user_waitlist(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<InvitationsWaitlistQueryParams>,
) -> ApiResult<PaginatedResponse<DeploymentWaitlistUser>> {
    let limit = params.limit.unwrap_or(10) as i32;

    let waitlist = DeploymentWaitlistQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(params.offset.unwrap_or(0))
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .execute(&app_state)
        .await
        .unwrap();

    let has_more = waitlist.len() > limit as usize;
    let waitlist = if has_more {
        waitlist[..limit as usize].to_vec()
    } else {
        waitlist
    };

    Ok(PaginatedResponse::from(waitlist).into())
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
    mut multipart: Multipart,
) -> ApiResult<UserWithIdentifiers> {
    let mut request = CreateUserRequest {
        first_name: String::new(),
        last_name: String::new(),
        email_address: None,
        phone_number: None,
        username: None,
        password: None,
    };

    let mut profile_image_data: Option<(Vec<u8>, String)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.name().unwrap_or_default().to_string();

        match name.as_str() {
            "first_name" => {
                request.first_name = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "last_name" => {
                request.last_name = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "email_address" => {
                let email = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !email.trim().is_empty() {
                    request.email_address = Some(email.trim().to_string());
                }
            }
            "phone_number" => {
                let phone = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !phone.trim().is_empty() {
                    request.phone_number = Some(phone.trim().to_string());
                }
            }
            "username" => {
                let username = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !username.trim().is_empty() {
                    request.username = Some(username.trim().to_string());
                }
            }
            "password" => {
                let password = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !password.trim().is_empty() {
                    request.password = Some(password.trim().to_string());
                }
            }
            "profile_image" => {
                let content_type = field.content_type().unwrap_or_default().to_string();

                if content_type.starts_with("image/") {
                    let file_extension = if content_type == "image/jpeg"
                        || content_type == "image/jpg"
                    {
                        "jpg"
                    } else if content_type == "image/png" {
                        "png"
                    } else if content_type == "image/gif" {
                        "gif"
                    } else if content_type == "image/webp" {
                        "webp"
                    } else if content_type == "image/x-icon"
                        || content_type == "image/vnd.microsoft.icon"
                    {
                        "ico"
                    } else {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            "Unsupported image format. Supported formats: JPEG, PNG, GIF, WEBP, ICO".to_string(),
                        ).into());
                    };

                    let image_buffer = field
                        .bytes()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                        .to_vec();

                    if !image_buffer.is_empty() {
                        profile_image_data = Some((image_buffer, file_extension.to_string()));
                    }
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
    mut multipart: Multipart,
) -> ApiResult<UserDetails> {
    let mut request = UpdateUserRequest {
        first_name: None,
        last_name: None,
        username: None,
        public_metadata: None,
        private_metadata: None,
    };

    let mut profile_image_data: Option<(Vec<u8>, String)> = None;

    // Parse multipart form data
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.name().unwrap_or_default().to_string();

        match name.as_str() {
            "first_name" => {
                let first_name = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !first_name.is_empty() {
                    request.first_name = Some(first_name);
                }
            }
            "last_name" => {
                let last_name = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !last_name.is_empty() {
                    request.last_name = Some(last_name);
                }
            }
            "username" => {
                let username = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !username.is_empty() {
                    request.username = Some(username);
                }
            }
            "public_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.is_empty() {
                    if let Ok(metadata) = serde_json::from_str(&metadata_str) {
                        request.public_metadata = Some(metadata);
                    }
                }
            }
            "private_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.is_empty() {
                    if let Ok(metadata) = serde_json::from_str(&metadata_str) {
                        request.private_metadata = Some(metadata);
                    }
                }
            }
            "profile_image" => {
                let content_type = field.content_type().unwrap_or_default().to_string();

                if content_type.starts_with("image/") {
                    let file_extension = if content_type == "image/jpeg"
                        || content_type == "image/jpg"
                    {
                        "jpg"
                    } else if content_type == "image/png" {
                        "png"
                    } else if content_type == "image/gif" {
                        "gif"
                    } else if content_type == "image/webp" {
                        "webp"
                    } else if content_type == "image/x-icon"
                        || content_type == "image/vnd.microsoft.icon"
                    {
                        "ico"
                    } else {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            "Unsupported image format. Supported formats: JPEG, PNG, GIF, WEBP, ICO".to_string(),
                        ).into());
                    };

                    let image_buffer = field
                        .bytes()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                        .to_vec();

                    if !image_buffer.is_empty() {
                        profile_image_data = Some((image_buffer, file_extension.to_string()));
                    }
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

pub async fn invite_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<InviteUserRequest>,
) -> ApiResult<DeploymentInvitation> {
    InviteUserCommand::new(deployment_id, request)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn approve_waitlist_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WaitlistUserParams>,
) -> ApiResult<DeploymentInvitation> {
    ApproveWaitlistUserCommand::new(deployment_id, params.waitlist_user_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn add_user_email(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(request): Json<AddEmailRequest>,
) -> ApiResult<UserEmailAddress> {
    AddUserEmailCommand::new(deployment_id, params.user_id, request)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_user_email(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserEmailParams>,
    Json(request): Json<UpdateEmailRequest>,
) -> ApiResult<UserEmailAddress> {
    UpdateUserEmailCommand::new(deployment_id, params.user_id, params.email_id, request)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_user_email(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserEmailParams>,
) -> ApiResult<()> {
    DeleteUserEmailCommand::new(params.user_id, params.email_id)
        .execute(&app_state)
        .await
        .unwrap();

    Ok(().into())
}

pub async fn add_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(request): Json<AddPhoneRequest>,
) -> ApiResult<UserPhoneNumber> {
    AddUserPhoneCommand::new(deployment_id, params.user_id, request)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserPhoneParams>,
    Json(request): Json<UpdatePhoneRequest>,
) -> ApiResult<UserPhoneNumber> {
    UpdateUserPhoneCommand::new(params.user_id, params.phone_id, request)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_user_phone(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserPhoneParams>,
) -> ApiResult<()> {
    DeleteUserPhoneCommand::new(params.user_id, params.phone_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_user_social_connection(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<UserSocialParams>,
) -> ApiResult<()> {
    DeleteUserSocialConnectionCommand::new(params.user_id, params.connection_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_user_password(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(new_password): Json<String>,
) -> ApiResult<()> {
    UpdateUserPasswordCommand::new(deployment_id, params.user_id, new_password)
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
        .map(Into::into)
        .map_err(Into::into)
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
