use anyhow::Result;
use chrono::{self, Datelike, Utc};
use commands::email::SendEmailCommand;
use common::{db_router::ReadConsistency, state::AppState};
use models::{DeploymentInvitation, DeploymentWithSettings, EmailProvider, SignIn, UserDetails};
use queries::{
    deployment::GetDeploymentWithSettingsQuery, invitation::GetDeploymentInvitationQuery,
    signin::GetSignInQuery, user::GetUserDetailsQuery, workspace::GetWorkspaceNameQuery,
};
use serde::{Deserialize, Serialize};

async fn run_email_command(
    command: SendEmailCommand,
    app_state: &AppState,
    error_prefix: &str,
) -> Result<(), String> {
    let email_deps = common::deps::from_app(app_state).db().enc().postmark().template();
    command
        .execute_with_deps(&email_deps)
        .await
        .map_err(|e| format!("{error_prefix}: {}", e))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VerificationEmailTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
    pub verification_code: String,
    pub ip_address: String,
    pub user_agent: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PasswordResetEmailTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
    pub reset_code: String,
    pub ip_address: String,
    pub user_agent: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MagicLinkEmailTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
    pub magic_link: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignInNotificationTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
    pub signin_id: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EmailChangeNotificationTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
    pub old_email: String,
    pub new_email: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PasswordChangeNotificationTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PasswordRemoveNotificationTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WaitlistSignupTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub first_name: String,
    pub last_name: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OrganizationMembershipInviteTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub inviter_name: String,
    pub organization_name: String,
    pub invite_link: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DeploymentInviteTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub inviter_user_id: u64,
    pub deployment_invitation_id: u64,
    pub workspace_id: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WaitlistApprovalTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub deployment_invitation_id: u64,
}

pub async fn send_verification_email_impl(
    deployment_id: u64,
    recipient: &str,
    verification_code: &str,
    ip_address: &str,
    user_agent: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let variables = create_verification_variables(
        &deployment_settings,
        verification_code,
        ip_address,
        user_agent,
        app_logo_url,
    );

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "verification_code_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send verification email").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!("verification_email_sent_{}", deployment_id))
}

pub async fn send_password_reset_email_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    reset_code: &str,
    ip_address: &str,
    user_agent: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = fetch_user_details(app_state, deployment_id, user_id).await?;

    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let variables = create_password_reset_variables(
        &user_details,
        &deployment_settings,
        reset_code,
        ip_address,
        user_agent,
        app_logo_url,
    );

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "reset_password_code_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send password reset email").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!("password_reset_email_sent_{}", deployment_id))
}

pub async fn send_magic_link_email_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    magic_link: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = fetch_user_details(app_state, deployment_id, user_id).await?;

    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let variables = create_magic_link_variables(
        &user_details,
        &deployment_settings,
        magic_link,
        app_logo_url,
    );

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "magic_link_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send magic link email").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!("magic_link_email_sent_{}", deployment_id))
}

pub async fn send_signin_notification_email_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    signin_id: u64,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = fetch_user_details(app_state, deployment_id, user_id).await?;

    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let signin_details = fetch_signin_details(&app_state, signin_id).await.ok();

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let variables = create_signin_notification_variables(
        &user_details,
        &deployment_settings,
        signin_details.as_ref(),
        app_logo_url,
    );

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "sign_in_from_new_device_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send signin notification email").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!("signin_notification_email_sent_{}", deployment_id))
}

pub async fn send_email_change_notification_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    old_email: &str,
    new_email: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = fetch_user_details(app_state, deployment_id, user_id).await?;

    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let variables = create_email_change_variables(
        &user_details,
        &deployment_settings,
        old_email,
        new_email,
        app_logo_url,
    );

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "primary_email_change_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send email change notification").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!("email_change_notification_sent_{}", deployment_id))
}

pub async fn send_password_change_notification_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = fetch_user_details(app_state, deployment_id, user_id).await?;

    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let variables =
        create_password_change_variables(&user_details, &deployment_settings, app_logo_url);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "password_change_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send password change notification").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!(
        "password_change_notification_sent_{}",
        deployment_id
    ))
}

pub async fn send_password_remove_notification_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = fetch_user_details(app_state, deployment_id, user_id).await?;

    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let variables =
        create_password_remove_variables(&user_details, &deployment_settings, app_logo_url);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "password_remove_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send password remove notification").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!(
        "password_remove_notification_sent_{}",
        deployment_id
    ))
}

pub async fn send_waitlist_signup_email_impl(
    deployment_id: u64,
    recipient: &str,
    _first_name: &str,
    _last_name: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_name = get_app_name_with_fallback(&deployment_settings);
    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());

    let variables = serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        }
    });

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "waitlist_signup_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send waitlist signup email").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!("waitlist_signup_email_sent_{}", deployment_id))
}

pub async fn send_organization_membership_invite_impl(
    deployment_id: u64,
    recipient: &str,
    inviter_name: &str,
    organization_name: &str,
    invite_link: &str,
    app_state: &AppState,
) -> Result<String, String> {
    // Fetch deployment settings
    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_name = get_app_name_with_fallback(&deployment_settings);
    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());

    let first_name = inviter_name
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();

    let variables = serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "inviter_name": inviter_name,
        "first_name": first_name,
        "organization_name": organization_name,
        "action_url": invite_link,
        "invitation": {
            "expires_in_days": "7"
        }
    });

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "organization_invite_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send organization invite email").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!(
        "organization_membership_invite_sent_{}",
        deployment_id
    ))
}

pub async fn send_deployment_invite_impl(
    deployment_id: u64,
    recipient: &str,
    inviter_user_id: u64,
    deployment_invitation_id: u64,
    workspace_id: Option<u64>,
    app_state: &AppState,
) -> Result<String, String> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let inviter_details = GetUserDetailsQuery::new(deployment_id as i64, inviter_user_id as i64)
        .execute_with_db(reader)
        .await
        .map_err(|e| format!("Failed to fetch inviter user details: {}", e))?;

    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let workspace_name = if let Some(ws_id) = workspace_id {
        fetch_workspace_name(&app_state, ws_id)
            .await
            .unwrap_or_else(|_| "Workspace".to_string())
    } else {
        "Workspace".to_string()
    };

    let invitation = fetch_deployment_invitation(&app_state, deployment_invitation_id)
        .await
        .map_err(|e| format!("Failed to fetch invitation: {}", e))?;

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let mut variables = create_workspace_invite_variables(
        &inviter_details,
        &deployment_settings,
        &workspace_name,
        Some(&invitation),
        app_logo_url,
    );

    let frontend_host = deployment_settings.frontend_host.clone();
    let action_url = format!(
        "https://{}/sign-up?invite_token={}",
        frontend_host, invitation.token
    );

    if let serde_json::Value::Object(ref mut map) = variables {
        map.insert(
            "action_url".to_string(),
            serde_json::Value::String(action_url),
        );
    }

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "workspace_invite_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send workspace invite email").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!("deployment_invite_sent_{}", deployment_id))
}

pub async fn send_waitlist_approval_impl(
    deployment_id: u64,
    recipient: &str,
    deployment_invitation_id: u64,
    app_state: &AppState,
) -> Result<String, String> {
    let invitation = fetch_deployment_invitation(&app_state, deployment_invitation_id)
        .await
        .map_err(|e| format!("Failed to fetch invitation: {}", e))?;

    let deployment_settings = fetch_deployment_settings(app_state, deployment_id).await?;

    let app_logo_url = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone());
    let mut variables =
        create_waitlist_invite_variables(&deployment_settings, Some(&invitation), app_logo_url);

    let frontend_host = deployment_settings.frontend_host.clone();
    let action_url = format!(
        "https://{}/sign-up?invite_token={}",
        frontend_host, invitation.token
    );

    if let serde_json::Value::Object(ref mut map) = variables {
        map.insert(
            "action_url".to_string(),
            serde_json::Value::String(action_url),
        );
    }

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "waitlist_invite_template".to_string(),
        recipient.to_string(),
        variables,
    );

    run_email_command(command, app_state, "Failed to send waitlist invite email").await?;

    if should_count_email_usage(&deployment_settings) {
        track_email_billing(deployment_id as i64, &app_state.redis_client).await;
    }

    Ok(format!("waitlist_approval_sent_{}", deployment_id))
}

async fn fetch_signin_details(app_state: &AppState, signin_id: u64) -> Result<SignIn, String> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetSignInQuery::new(signin_id as i64)
        .execute_with_db(reader)
        .await
        .map_err(|e| format!("Failed to fetch signin details: {}", e))
}

async fn fetch_deployment_settings(
    app_state: &AppState,
    deployment_id: u64,
) -> Result<DeploymentWithSettings, String> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute_with_db(reader)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))
}

async fn fetch_deployment_invitation(
    app_state: &AppState,
    deployment_invitation_id: u64,
) -> Result<DeploymentInvitation, String> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetDeploymentInvitationQuery::new(deployment_invitation_id as i64)
        .execute_with_db(reader)
        .await
        .map_err(|e| format!("Failed to fetch deployment invitation: {}", e))
}

async fn fetch_workspace_name(app_state: &AppState, workspace_id: u64) -> Result<String, String> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetWorkspaceNameQuery::new(workspace_id as i64)
        .execute_with_db(reader)
        .await
        .map_err(|e| format!("Failed to fetch workspace name: {}", e))
}

async fn fetch_user_details(
    app_state: &AppState,
    deployment_id: u64,
    user_id: u64,
) -> Result<UserDetails, String> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute_with_db(reader)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))
}

fn create_verification_variables(
    deployment: &DeploymentWithSettings,
    verification_code: &str,
    ip_address: &str,
    user_agent: &str,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    let device_info = if !user_agent.is_empty() {
        format!("{} (IP: {})", user_agent, ip_address)
    } else {
        format!("IP: {}", ip_address)
    };

    serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "code": {
            "value": verification_code,
            "expires_in_minutes": "15"
        },
        "device": {
            "info": device_info,
            "ip_address": ip_address,
            "user_agent": user_agent
        }
    })
}

fn create_password_reset_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    reset_code: &str,
    ip_address: &str,
    user_agent: &str,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    let device_info = if !user_agent.is_empty() {
        format!("{} (IP: {})", user_agent, ip_address)
    } else {
        format!("IP: {}", ip_address)
    };

    serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "user": {
            "id": user.id.to_string(),
            "first_name": user.first_name,
            "last_name": user.last_name,
            "full_name": format!("{} {}", user.first_name, user.last_name),
            "username": user.username,
            "email": user.primary_email_address,
            "phone": user.primary_phone_number,
            "profile_picture_url": user.profile_picture_url,
            "created_at": user.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "disabled": user.disabled,
            "has_password": user.has_password,
            "public_metadata": user.public_metadata,
            "private_metadata": user.private_metadata
        },
        "code": {
            "value": reset_code,
            "expires_in_minutes": "15"
        },
        "device": {
            "info": device_info,
            "ip_address": ip_address,
            "user_agent": user_agent
        }
    })
}

fn create_signin_notification_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    signin: Option<&SignIn>,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    let mut json_value = serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "user": {
            "id": user.id.to_string(),
            "first_name": user.first_name,
            "last_name": user.last_name,
            "full_name": format!("{} {}", user.first_name, user.last_name),
            "username": user.username,
            "email": user.primary_email_address,
            "phone": user.primary_phone_number,
            "profile_picture_url": user.profile_picture_url,
            "created_at": user.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "disabled": user.disabled,
            "has_password": user.has_password,
            "public_metadata": user.public_metadata,
            "private_metadata": user.private_metadata
        }
    });

    if let Some(signin) = signin {
        let location = if !signin.city.is_empty() && !signin.country.is_empty() {
            format!("{}, {}", signin.city, signin.country)
        } else if !signin.region.is_empty() && !signin.country.is_empty() {
            format!("{}, {}", signin.region, signin.country)
        } else if !signin.country.is_empty() {
            signin.country.clone()
        } else {
            "Unknown Location".to_string()
        };

        let device_name = if signin.device.is_empty() {
            "Unknown Device".to_string()
        } else {
            signin.device.clone()
        };

        let device_info = format!("{} (IP: {})", device_name, signin.ip_address);

        json_value["signin"] = serde_json::json!({
            "time": signin.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "location": location
        });

        json_value["device"] = serde_json::json!({
            "info": device_info,
            "name": device_name,
            "browser": signin.browser,
            "ip_address": signin.ip_address
        });
    } else {
        json_value["device"] = serde_json::json!({
            "info": "Unknown Device",
            "name": "Unknown Device",
            "browser": "Unknown Browser",
            "ip_address": "Unknown IP"
        });
        json_value["signin"] = serde_json::json!({
            "time": "",
            "location": "Unknown Location"
        });
    }

    json_value
}

fn create_email_change_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    old_email: &str,
    new_email: &str,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "user": {
            "id": user.id.to_string(),
            "first_name": user.first_name,
            "last_name": user.last_name,
            "full_name": format!("{} {}", user.first_name, user.last_name),
            "username": user.username,
            "email": user.primary_email_address,
            "phone": user.primary_phone_number,
            "profile_picture_url": user.profile_picture_url,
            "created_at": user.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "disabled": user.disabled,
            "has_password": user.has_password,
            "public_metadata": user.public_metadata,
            "private_metadata": user.private_metadata
        },
        "old_email": old_email,
        "new_email": new_email
    })
}

fn create_password_change_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "user": {
            "id": user.id.to_string(),
            "first_name": user.first_name,
            "last_name": user.last_name,
            "full_name": format!("{} {}", user.first_name, user.last_name),
            "username": user.username,
            "email": user.primary_email_address,
            "phone": user.primary_phone_number,
            "profile_picture_url": user.profile_picture_url,
            "created_at": user.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "disabled": user.disabled,
            "has_password": user.has_password,
            "public_metadata": user.public_metadata,
            "private_metadata": user.private_metadata
        },
        "change_time": chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
    })
}

fn create_password_remove_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "user": {
            "id": user.id.to_string(),
            "first_name": user.first_name,
            "last_name": user.last_name,
            "full_name": format!("{} {}", user.first_name, user.last_name),
            "username": user.username,
            "email": user.primary_email_address,
            "phone": user.primary_phone_number,
            "profile_picture_url": user.profile_picture_url,
            "created_at": user.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "disabled": user.disabled,
            "has_password": user.has_password,
            "public_metadata": user.public_metadata,
            "private_metadata": user.private_metadata
        },
        "removal_time": chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
    })
}

fn get_app_name_with_fallback(deployment: &DeploymentWithSettings) -> String {
    deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "".to_string())
}

fn create_waitlist_invite_variables(
    deployment: &DeploymentWithSettings,
    invitation: Option<&DeploymentInvitation>,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    let action_url = if let Some(invitation) = invitation {
        format!(
            "https://{}/sign-up?invite_token={}",
            deployment.frontend_host, invitation.token
        )
    } else {
        "".to_string()
    };

    serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "action_url": action_url
    })
}

fn create_workspace_invite_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    workspace_name: &str,
    invitation: Option<&DeploymentInvitation>,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    let (expires_in_days, expiry_date) = if let Some(invitation) = invitation {
        let days_until_expiry = (invitation.expiry - chrono::Utc::now()).num_days();
        (
            days_until_expiry.max(0).to_string(),
            invitation.expiry.format("%Y-%m-%d").to_string(),
        )
    } else {
        ("7".to_string(), "".to_string())
    };

    serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "user": {
            "id": user.id.to_string(),
            "first_name": user.first_name,
            "last_name": user.last_name,
            "full_name": format!("{} {}", user.first_name, user.last_name),
            "username": user.username,
            "email": user.primary_email_address,
            "phone": user.primary_phone_number,
            "profile_picture_url": user.profile_picture_url,
            "created_at": user.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "disabled": user.disabled,
            "has_password": user.has_password,
            "public_metadata": user.public_metadata,
            "private_metadata": user.private_metadata
        },
        "workspace_name": workspace_name,
        "inviter_name": format!("{} {}", user.first_name, user.last_name),
        "invitation": {
            "expires_in_days": expires_in_days,
            "expiry": expiry_date
        }
    })
}

fn create_magic_link_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    magic_link: &str,
    app_logo_url: Option<String>,
) -> serde_json::Value {
    let app_name = get_app_name_with_fallback(deployment);

    serde_json::json!({
        "app": {
            "name": app_name,
            "logo": app_logo_url
        },
        "user": {
            "id": user.id.to_string(),
            "first_name": user.first_name,
            "last_name": user.last_name,
            "full_name": format!("{} {}", user.first_name, user.last_name),
            "username": user.username,
            "email": user.primary_email_address,
            "phone": user.primary_phone_number,
            "profile_picture_url": user.profile_picture_url,
            "created_at": user.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "disabled": user.disabled,
            "has_password": user.has_password,
            "public_metadata": user.public_metadata,
            "private_metadata": user.private_metadata
        },
        "action_url": magic_link,
        "link": {
            "expires_in_minutes": "15"
        }
    })
}

fn should_count_email_usage(deployment_settings: &DeploymentWithSettings) -> bool {
    deployment_settings.email_provider != EmailProvider::CustomSmtp
}

async fn track_email_billing(deployment_id: i64, redis_client: &redis::Client) {
    if let Ok(mut conn) = redis_client.get_multiplexed_async_connection().await {
        let now = Utc::now();
        let period = format!("{}-{:02}", now.year(), now.month());
        let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

        let mut pipe = redis::pipe();
        pipe.atomic()
            .zincr(&format!("{}:metrics", prefix), "emails", 1)
            .ignore()
            .expire(&format!("{}:metrics", prefix), 5184000)
            .ignore()
            .zincr(
                &format!("billing:{}:dirty_deployments", period),
                deployment_id,
                1,
            )
            .ignore()
            .expire(&format!("billing:{}:dirty_deployments", period), 5184000)
            .ignore();

        let _: Result<(), redis::RedisError> = pipe.query_async(&mut conn).await;
    }
}
