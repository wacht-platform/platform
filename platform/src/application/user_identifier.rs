use commands::{
    AddUserEmailCommand, AddUserPhoneCommand, DeleteUserEmailCommand, DeleteUserPhoneCommand,
    DeleteUserSocialConnectionCommand, UpdateUserEmailCommand, UpdateUserPhoneCommand,
};
use common::error::AppError;
use common::state::AppState;
use dto::json::{AddEmailRequest, AddPhoneRequest, UpdateEmailRequest, UpdatePhoneRequest};
use models::{UserEmailAddress, UserPhoneNumber};

pub async fn add_user_email(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    request: AddEmailRequest,
) -> Result<UserEmailAddress, AppError> {
    AddUserEmailCommand::new(deployment_id, user_id, request)
        .execute_with(app_state.db_router.writer(), app_state.sf.next_id()? as i64)
        .await
}

pub async fn update_user_email(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    email_id: i64,
    request: UpdateEmailRequest,
) -> Result<UserEmailAddress, AppError> {
    UpdateUserEmailCommand::new(deployment_id, user_id, email_id, request)
        .execute_with(app_state.db_router.writer())
        .await
}

pub async fn delete_user_email(
    app_state: &AppState,
    user_id: i64,
    email_id: i64,
) -> Result<(), AppError> {
    DeleteUserEmailCommand::new(user_id, email_id)
        .execute_with(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn add_user_phone(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    request: AddPhoneRequest,
) -> Result<UserPhoneNumber, AppError> {
    AddUserPhoneCommand::new(deployment_id, user_id, request)
        .execute_with(app_state.db_router.writer(), app_state.sf.next_id()? as i64)
        .await
}

pub async fn update_user_phone(
    app_state: &AppState,
    user_id: i64,
    phone_id: i64,
    request: UpdatePhoneRequest,
) -> Result<UserPhoneNumber, AppError> {
    UpdateUserPhoneCommand::new(user_id, phone_id, request)
        .execute_with(app_state.db_router.writer())
        .await
}

pub async fn delete_user_phone(
    app_state: &AppState,
    user_id: i64,
    phone_id: i64,
) -> Result<(), AppError> {
    DeleteUserPhoneCommand::new(user_id, phone_id)
        .execute_with(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn delete_user_social_connection(
    app_state: &AppState,
    user_id: i64,
    connection_id: i64,
) -> Result<(), AppError> {
    DeleteUserSocialConnectionCommand::new(user_id, connection_id)
        .execute_with(app_state.db_router.writer())
        .await?;
    Ok(())
}
