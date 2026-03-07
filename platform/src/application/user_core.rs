use commands::{
    CreateUserCommand, DeleteUserCommand, GenerateImpersonationTokenCommand, UpdateUserCommand,
    UpdateUserPasswordCommand, UpdateUserProfileImageCommand, UploadToCdnCommand,
};
use common::db_router::ReadConsistency;
use common::error::AppError;
use common::state::AppState;
use dto::{
    json::{CreateUserRequest, UpdatePasswordRequest, UpdateUserRequest},
    query::ActiveUserListQueryParams,
};
use models::{UserDetails, UserWithIdentifiers};
use queries::{DeploymentActiveUserListQuery, GetUserDetailsQuery};

use crate::{api::pagination::paginate_results, application::response::PaginatedResponse};

pub async fn get_active_user_list(
    app_state: &AppState,
    deployment_id: i64,
    params: ActiveUserListQueryParams,
) -> Result<PaginatedResponse<UserWithIdentifiers>, AppError> {
    let limit = params.limit.unwrap_or(10) as i32;
    let offset = params.offset.unwrap_or(0);

    let users = DeploymentActiveUserListQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(offset)
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute_with(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;

    Ok(paginate_results(users, limit, Some(offset)))
}

pub async fn get_user_details(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<UserDetails, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetUserDetailsQuery::new(deployment_id, user_id)
        .execute_with(reader)
        .await
}

pub async fn create_user(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateUserRequest,
    profile_image_data: Option<(Vec<u8>, String)>,
) -> Result<UserWithIdentifiers, AppError> {
    let user = CreateUserCommand::new(deployment_id, request)
        .execute_with_deps(app_state)
        .await?;

    if let Some((image_buffer, file_extension)) = profile_image_data {
        let url = upload_user_profile_image(
            app_state,
            deployment_id,
            user.id,
            image_buffer,
            file_extension,
        )
        .await?;

        UpdateUserProfileImageCommand::new(deployment_id, user.id, url)
            .execute_with(app_state.db_router.writer())
            .await?;
    }

    Ok(user)
}

pub async fn update_user(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    request: UpdateUserRequest,
    profile_image_data: Option<(Vec<u8>, String)>,
    remove_profile_image: bool,
) -> Result<UserDetails, AppError> {
    let user_details = UpdateUserCommand::new(deployment_id, user_id, request)
        .execute_with_deps(app_state)
        .await?;

    if remove_profile_image {
        UpdateUserProfileImageCommand::new(deployment_id, user_id, String::new())
            .execute_with(app_state.db_router.writer())
            .await?;

        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        return GetUserDetailsQuery::new(deployment_id, user_id)
            .execute_with(reader)
            .await;
    }

    if let Some((image_buffer, file_extension)) = profile_image_data {
        let url = upload_user_profile_image(
            app_state,
            deployment_id,
            user_id,
            image_buffer,
            file_extension,
        )
        .await?;

        UpdateUserProfileImageCommand::new(deployment_id, user_id, url)
            .execute_with(app_state.db_router.writer())
            .await?;

        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        return GetUserDetailsQuery::new(deployment_id, user_id)
            .execute_with(reader)
            .await;
    }

    Ok(user_details)
}

pub async fn update_user_password(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    request: UpdatePasswordRequest,
) -> Result<(), AppError> {
    let password_command = UpdateUserPasswordCommand::new(
        deployment_id,
        user_id,
        request.new_password,
        request.skip_password_check,
    );
    password_command.execute_with_deps(app_state).await?;
    Ok(())
}

pub async fn delete_user(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<(), AppError> {
    DeleteUserCommand::new(deployment_id, user_id)
        .execute_with(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn impersonate_user(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<commands::GenerateImpersonationTokenResponse, AppError> {
    GenerateImpersonationTokenCommand::new(deployment_id, user_id)
        .execute_with(app_state.db_router.writer())
        .await
}

async fn upload_user_profile_image(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    image_buffer: Vec<u8>,
    file_extension: String,
) -> Result<String, AppError> {
    let file_path = format!(
        "deployments/{}/users/{}/profile.{}",
        deployment_id, user_id, file_extension
    );

    UploadToCdnCommand::new(file_path, image_buffer)
        .execute_with_deps(&app_state.s3_client)
        .await
}
