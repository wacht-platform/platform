use commands::{
    CreateUserAuthenticatorCommand, CreateUserAuthenticatorResponse, CreateUserCommand,
    DeleteUserAuthenticatorCommand, DeleteUserCommand, DeleteUserPasskeyCommand,
    GenerateImpersonationTokenCommand, MakeUserEmailPrimaryCommand, MakeUserPhonePrimaryCommand,
    RegenerateUserBackupCodesCommand, RemoveUserPasswordCommand, RenameUserPasskeyCommand,
    RevokeAllUserSigninsCommand, RevokeUserSigninCommand, UpdateUserCommand,
    UpdateUserPasswordCommand, UpdateUserProfileImageCommand, UploadToCdnCommand,
};
use common::db_router::ReadConsistency;
use common::error::AppError;
use common::state::AppState;
use chrono::Utc;
use dto::{
    json::{CreateUserRequest, NatsTaskMessage, UpdatePasswordRequest, UpdateUserRequest},
    query::ActiveUserListQueryParams,
};
use serde_json::json;
use tracing::warn;
use models::{
    SignIn, SocialConnection, UserDetails, UserOrganizationMembership, UserPasskey,
    UserWithIdentifiers, UserWorkspaceMembership,
};
use queries::{
    DeploymentActiveUserListQuery, GetUserDetailsQuery, GetUserOrganizationMembershipsQuery,
    GetUserPasskeysQuery, GetUserSigninsQuery, GetUserSocialConnectionsQuery,
    GetUserWorkspaceMembershipsQuery,
};

use crate::{api::pagination::paginate_results, application::response::PaginatedResponse};
use common::deps;

const WEBHOOK_EVENT_SUBJECT: &str = "worker.tasks.webhook.event";

// Fire-and-forget publish of a `user.*` webhook event onto the same NATS
// subject that the frontend API uses, so subscribers see admin-driven user
// mutations identically to self-service ones. Failure to publish must not
// fail the originating mutation — log and move on, matching FAPI semantics.
fn publish_user_webhook_event(
    app_state: &AppState,
    deployment_id: i64,
    event_type: &'static str,
    entity_id: i64,
    entity_type: &'static str,
) {
    let task_id = match app_state.sf.next_id() {
        Ok(id) => id.to_string(),
        Err(e) => {
            warn!(error = %e, event_type, "Failed to allocate webhook task id");
            return;
        }
    };

    let inner = json!({
        "deployment_id": deployment_id,
        "event_type": event_type,
        "event_payload": {
            "entity_id": entity_id,
            "entity_type": entity_type,
        },
        "triggered_at": Utc::now(),
    });

    let envelope = NatsTaskMessage {
        task_type: "webhook.event".to_string(),
        task_id,
        payload: inner,
    };

    let bytes = match serde_json::to_vec(&envelope) {
        Ok(b) => b,
        Err(e) => {
            warn!(error = %e, event_type, "Failed to serialize webhook task");
            return;
        }
    };

    let nats = app_state.nats_client.clone();
    tokio::spawn(async move {
        if let Err(e) = nats.publish(WEBHOOK_EVENT_SUBJECT, bytes.into()).await {
            warn!(error = %e, event_type, "Failed to publish webhook event");
        }
    });
}

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
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;

    Ok(paginate_results(users, limit, Some(offset)))
}

pub async fn get_user_details(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<UserDetails, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let mut details = GetUserDetailsQuery::new(deployment_id, user_id)
        .execute_with_db(reader)
        .await?;
    decrypt_social_connections(app_state, &mut details.social_connections)?;
    Ok(details)
}

fn decrypt_social_connections(
    app_state: &AppState,
    connections: &mut [models::SocialConnection],
) -> Result<(), AppError> {
    for sc in connections.iter_mut() {
        if !sc.access_token.is_empty() {
            sc.access_token = app_state.encryption_service.decrypt(&sc.access_token)?;
        }
        if !sc.refresh_token.is_empty() {
            sc.refresh_token = app_state.encryption_service.decrypt(&sc.refresh_token)?;
        }
    }
    Ok(())
}

pub async fn create_user(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateUserRequest,
    profile_image_data: Option<(Vec<u8>, String)>,
) -> Result<UserWithIdentifiers, AppError> {
    let deps = deps::from_app(app_state).db().id();
    let user = CreateUserCommand::new(deployment_id, request)
        .execute_with_deps(&deps)
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
            .execute_with_db(app_state.db_router.writer())
            .await?;
    }

    publish_user_webhook_event(app_state, deployment_id, "user.created", user.id, "user");

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
    let deps = deps::from_app(app_state).db();
    let user_details = UpdateUserCommand::new(deployment_id, user_id, request)
        .execute_with_deps(&deps)
        .await?;

    // Fire once after the row mutation succeeds; profile-image side-effects
    // below are surfacing the same updated user and shouldn't double-emit.
    publish_user_webhook_event(app_state, deployment_id, "user.updated", user_id, "user");

    if remove_profile_image {
        UpdateUserProfileImageCommand::new(deployment_id, user_id, String::new())
            .execute_with_db(app_state.db_router.writer())
            .await?;

        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        return GetUserDetailsQuery::new(deployment_id, user_id)
            .execute_with_db(reader)
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
            .execute_with_db(app_state.db_router.writer())
            .await?;

        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        return GetUserDetailsQuery::new(deployment_id, user_id)
            .execute_with_db(reader)
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
    let deps = deps::from_app(app_state).db();
    password_command.execute_with_deps(&deps).await?;
    Ok(())
}

pub async fn delete_user(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<(), AppError> {
    DeleteUserCommand::new(deployment_id, user_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    publish_user_webhook_event(app_state, deployment_id, "user.deleted", user_id, "user");
    Ok(())
}

pub async fn impersonate_user(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<commands::GenerateImpersonationTokenResponse, AppError> {
    GenerateImpersonationTokenCommand::new(deployment_id, user_id)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn get_user_organization_memberships(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<Vec<UserOrganizationMembership>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetUserOrganizationMembershipsQuery::new(deployment_id, user_id)
        .execute_with_db(reader)
        .await
}

pub async fn get_user_workspace_memberships(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<Vec<UserWorkspaceMembership>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetUserWorkspaceMembershipsQuery::new(deployment_id, user_id)
        .execute_with_db(reader)
        .await
}

pub async fn get_user_signins(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    include_expired: bool,
) -> Result<Vec<SignIn>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetUserSigninsQuery::new(deployment_id, user_id)
        .include_expired(include_expired)
        .execute_with_db(reader)
        .await
}

pub async fn revoke_user_signin(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    signin_id: i64,
) -> Result<(), AppError> {
    RevokeUserSigninCommand::new(deployment_id, user_id, signin_id)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn revoke_all_user_signins(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<u64, AppError> {
    RevokeAllUserSigninsCommand::new(deployment_id, user_id)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn get_user_passkeys(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<Vec<UserPasskey>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetUserPasskeysQuery::new(deployment_id, user_id)
        .execute_with_db(reader)
        .await
}

pub async fn rename_user_passkey(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    passkey_id: i64,
    new_name: String,
) -> Result<(), AppError> {
    RenameUserPasskeyCommand::new(deployment_id, user_id, passkey_id, new_name)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn delete_user_passkey(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    passkey_id: i64,
) -> Result<(), AppError> {
    DeleteUserPasskeyCommand::new(deployment_id, user_id, passkey_id)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn create_user_authenticator(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    secret_base32: String,
    account_name: Option<String>,
) -> Result<CreateUserAuthenticatorResponse, AppError> {
    let authenticator_id = app_state.sf.next_id()? as i64;
    CreateUserAuthenticatorCommand::new(deployment_id, user_id, authenticator_id, secret_base32)
        .with_account_name(account_name)
        .execute_with_pool(app_state.db_router.writer(), &app_state.encryption_service)
        .await
}

pub async fn delete_user_authenticator(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<(), AppError> {
    DeleteUserAuthenticatorCommand::new(deployment_id, user_id)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn regenerate_user_backup_codes(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<Vec<String>, AppError> {
    RegenerateUserBackupCodesCommand::new(deployment_id, user_id)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn remove_user_password(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<(), AppError> {
    RemoveUserPasswordCommand::new(deployment_id, user_id)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn make_user_email_primary(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    email_id: i64,
) -> Result<(), AppError> {
    MakeUserEmailPrimaryCommand::new(deployment_id, user_id, email_id)
        .execute_with_pool(app_state.db_router.writer())
        .await
}

pub async fn make_user_phone_primary(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
    phone_id: i64,
) -> Result<(), AppError> {
    MakeUserPhonePrimaryCommand::new(deployment_id, user_id, phone_id)
        .execute_with_pool(app_state.db_router.writer())
        .await
}

pub async fn get_user_social_connections(
    app_state: &AppState,
    deployment_id: i64,
    user_id: i64,
) -> Result<Vec<SocialConnection>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let mut connections = GetUserSocialConnectionsQuery::new(deployment_id, user_id)
        .execute_with_db(reader)
        .await?;
    decrypt_social_connections(app_state, &mut connections)?;
    Ok(connections)
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

    let deps = deps::from_app(app_state).s3();
    UploadToCdnCommand::new(file_path, image_buffer)
        .execute_with_deps(&deps)
        .await
}
