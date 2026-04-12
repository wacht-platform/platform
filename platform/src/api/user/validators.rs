use crate::application::response::ApiErrorResponse;
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::{CreateUserRequest, UpdateUserRequest};
use queries::GetDeploymentAuthSettingsQuery;

async fn get_deployment_auth_settings(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<models::DeploymentAuthSettings, ApiErrorResponse> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetDeploymentAuthSettingsQuery::new(deployment_id)
        .execute_with_db(reader)
        .await
        .map_err(|e| {
            ApiErrorResponse::internal(format!("Failed to get deployment auth settings: {}", e))
        })
}

pub(super) async fn validate_create_user_request(
    app_state: &AppState,
    deployment_id: i64,
    request: &CreateUserRequest,
) -> Result<(), ApiErrorResponse> {
    let auth_settings = get_deployment_auth_settings(app_state, deployment_id).await?;

    if auth_settings.first_name.enabled
        && auth_settings.first_name.required.unwrap_or(true)
        && request.first_name.trim().is_empty()
    {
        return Err(ApiErrorResponse::bad_request("First name is required"));
    }

    if auth_settings.last_name.enabled
        && auth_settings.last_name.required.unwrap_or(true)
        && request.last_name.trim().is_empty()
    {
        return Err(ApiErrorResponse::bad_request("Last name is required"));
    }

    if auth_settings.email_address.enabled
        && auth_settings.email_address.required
        && request.email_address.is_none()
    {
        return Err(ApiErrorResponse::bad_request("Email address is required"));
    }

    if auth_settings.phone_number.enabled
        && auth_settings.phone_number.required
        && request.phone_number.is_none()
    {
        return Err(ApiErrorResponse::bad_request("Phone number is required"));
    }

    if auth_settings.username.enabled
        && auth_settings.username.required
        && request.username.is_none()
    {
        return Err(ApiErrorResponse::bad_request("Username is required"));
    }

    if auth_settings.password.enabled && !request.skip_password_check {
        if let Some(password) = &request.password {
            if let Some(min_length) = auth_settings.password.min_length {
                if password.len() < min_length as usize {
                    return Err(ApiErrorResponse::bad_request(format!(
                        "Password must be at least {} characters",
                        min_length
                    )));
                }
            }
        } else if auth_settings.password.min_length.unwrap_or(0) > 0 {
            return Err(ApiErrorResponse::bad_request("Password is required"));
        }
    }

    Ok(())
}

pub(super) async fn validate_update_user_request(
    app_state: &AppState,
    deployment_id: i64,
    request: &UpdateUserRequest,
) -> Result<(), ApiErrorResponse> {
    let auth_settings = get_deployment_auth_settings(app_state, deployment_id).await?;

    if let Some(first_name) = &request.first_name {
        if auth_settings.first_name.enabled
            && auth_settings.first_name.required.unwrap_or(true)
            && first_name.trim().is_empty()
        {
            return Err(ApiErrorResponse::bad_request("First name cannot be empty"));
        }
    }

    if let Some(last_name) = &request.last_name {
        if auth_settings.last_name.enabled
            && auth_settings.last_name.required.unwrap_or(true)
            && last_name.trim().is_empty()
        {
            return Err(ApiErrorResponse::bad_request("Last name cannot be empty"));
        }
    }

    if let Some(username) = &request.username {
        if auth_settings.username.enabled {
            if auth_settings.username.required && username.trim().is_empty() {
                return Err(ApiErrorResponse::bad_request("Username cannot be empty"));
            }

            let username_len = username.trim().len();
            if let Some(min_length) = auth_settings.username.min_length {
                if username_len < min_length as usize {
                    return Err(ApiErrorResponse::bad_request(format!(
                        "Username must be at least {} characters",
                        min_length
                    )));
                }
            }
            if let Some(max_length) = auth_settings.username.max_length {
                if username_len > max_length as usize {
                    return Err(ApiErrorResponse::bad_request(format!(
                        "Username must be at most {} characters",
                        max_length
                    )));
                }
            }
        }
    }

    Ok(())
}
