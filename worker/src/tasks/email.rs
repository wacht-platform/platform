use anyhow::Result;
use chrono;
use commands::{Command, email::SendEmailCommand};
use common::state::AppState;
use models::{
    DeploymentInvitation, DeploymentWithSettings, SchemaVersion, SecondFactorPolicy, SignIn,
    UserDetails,
};
use queries::{
    Query, deployment::GetDeploymentWithSettingsQuery, invitation::GetDeploymentInvitationQuery,
    organization::GetOrganizationNameQuery, signin::GetSignInQuery, user::GetUserDetailsQuery,
    workspace::GetWorkspaceNameQuery,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct VerificationEmailTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
    pub verification_code: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PasswordResetEmailTask {
    pub deployment_id: u64,
    pub recipient: String,
    pub user_id: u64,
    pub reset_code: String,
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
    pub user_id: u64,
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
    user_id: u64,
    verification_code: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))?;

    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let variables =
        create_verification_variables(&user_details, &deployment_settings, verification_code);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "verification_code_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send verification email: {}", e))?;

    Ok(format!("verification_email_sent_{}", deployment_id))
}

pub async fn send_password_reset_email_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    reset_code: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))?;

    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let variables =
        create_password_reset_variables(&user_details, &deployment_settings, reset_code);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "reset_password_code_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send password reset email: {}", e))?;

    Ok(format!("password_reset_email_sent_{}", deployment_id))
}

pub async fn send_magic_link_email_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    magic_link: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))?;

    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let variables = create_magic_link_variables(&user_details, &deployment_settings, magic_link);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "magic_link_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send magic link email: {}", e))?;

    Ok(format!("magic_link_email_sent_{}", deployment_id))
}

pub async fn send_signin_notification_email_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    signin_id: u64,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))?;

    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let signin_details = fetch_signin_details(&app_state, signin_id).await.ok();

    let variables = create_signin_notification_variables(
        &user_details,
        &deployment_settings,
        signin_details.as_ref(),
    );

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "sign_in_from_new_device_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send signin notification email: {}", e))?;

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
    let user_details = GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))?;

    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let variables =
        create_email_change_variables(&user_details, &deployment_settings, old_email, new_email);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "primary_email_change_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send email change notification: {}", e))?;

    Ok(format!("email_change_notification_sent_{}", deployment_id))
}

pub async fn send_password_change_notification_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))?;

    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let variables = create_password_change_variables(&user_details, &deployment_settings);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "password_change_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send password change notification: {}", e))?;

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
    let user_details = GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))?;

    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let variables = create_password_remove_variables(&user_details, &deployment_settings);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "password_remove_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send password remove notification: {}", e))?;

    Ok(format!(
        "password_remove_notification_sent_{}",
        deployment_id
    ))
}

pub async fn send_waitlist_signup_email_impl(
    deployment_id: u64,
    recipient: &str,
    user_id: u64,
    app_state: &AppState,
) -> Result<String, String> {
    let user_details = GetUserDetailsQuery::new(deployment_id as i64, user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch user details: {}", e))?;

    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let variables = create_waitlist_signup_variables(&user_details, &deployment_settings);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "waitlist_signup_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send waitlist signup email: {}", e))?;

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
    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    // Create variables directly without needing UserDetails
    let mut variables = HashMap::new();

    let app_name = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment_settings
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert(
        "first_name".to_string(),
        inviter_name
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string(),
    );
    variables.insert(
        "organization_name".to_string(),
        organization_name.to_string(),
    );
    variables.insert("inviter_name".to_string(), inviter_name.to_string());
    variables.insert("action_url".to_string(), invite_link.to_string());
    variables.insert("invitation.expires_in_days".to_string(), "7".to_string());

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "organization_invite_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send organization invite email: {}", e))?;

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
    // Fetch inviter user details
    let inviter_details = GetUserDetailsQuery::new(deployment_id as i64, inviter_user_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch inviter user details: {}", e))?;

    // Fetch deployment settings
    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    // Fetch workspace name if workspace_id is provided
    let workspace_name = if let Some(ws_id) = workspace_id {
        fetch_workspace_name(&app_state, ws_id)
            .await
            .unwrap_or_else(|_| "Workspace".to_string())
    } else {
        "Workspace".to_string()
    };

    // Fetch invitation details
    let invitation = fetch_deployment_invitation(&app_state, deployment_invitation_id)
        .await
        .map_err(|e| format!("Failed to fetch invitation: {}", e))?;

    let mut variables = create_workspace_invite_variables(
        &inviter_details,
        &deployment_settings,
        &workspace_name,
        Some(&invitation),
    );

    let frontend_host = deployment_settings.frontend_host.clone();
    let action_url = format!(
        "https://{}/sign-up?invite_token={}",
        frontend_host, invitation.token
    );
    variables.insert("action_url".to_string(), action_url);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "workspace_invite_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send workspace invite email: {}", e))?;

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

    // Fetch deployment settings
    let deployment_settings = GetDeploymentWithSettingsQuery::new(deployment_id as i64)
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment settings: {}", e))?;

    let user_details = UserDetails {
        id: 0,
        created_at: invitation.created_at,
        updated_at: invitation.updated_at,
        first_name: invitation.first_name.clone(),
        last_name: invitation.last_name.clone(),
        username: None,
        profile_picture_url: String::new(),
        schema_version: SchemaVersion::V1,
        disabled: false,
        second_factor_policy: SecondFactorPolicy::Optional,
        active_organization_membership_id: None,
        active_workspace_membership_id: None,
        deployment_id: deployment_id as i64,
        public_metadata: serde_json::Value::Null,
        private_metadata: serde_json::Value::Null,
        primary_email_address: Some(invitation.email_address.clone()),
        primary_phone_number: None,
        email_addresses: vec![],
        phone_numbers: vec![],
        social_connections: vec![],
        has_password: false,
        has_backup_codes: false,
    };

    let mut variables =
        create_waitlist_invite_variables(&user_details, &deployment_settings, Some(&invitation));

    let frontend_host = deployment_settings.frontend_host.clone();
    let action_url = format!(
        "https://{}/sign-up?invite_token={}",
        frontend_host, invitation.token
    );
    variables.insert("action_url".to_string(), action_url);

    let command = SendEmailCommand::new(
        deployment_id as i64,
        "waitlist_invite_template".to_string(),
        recipient.to_string(),
        variables,
    );

    command
        .execute(&app_state)
        .await
        .map_err(|e| format!("Failed to send waitlist invite email: {}", e))?;

    Ok(format!("waitlist_approval_sent_{}", deployment_id))
}

async fn fetch_signin_details(app_state: &AppState, signin_id: u64) -> Result<SignIn, String> {
    GetSignInQuery::new(signin_id as i64)
        .execute(app_state)
        .await
        .map_err(|e| format!("Failed to fetch signin details: {}", e))
}

async fn fetch_deployment_invitation(
    app_state: &AppState,
    deployment_invitation_id: u64,
) -> Result<DeploymentInvitation, String> {
    GetDeploymentInvitationQuery::new(deployment_invitation_id as i64)
        .execute(app_state)
        .await
        .map_err(|e| format!("Failed to fetch deployment invitation: {}", e))
}

async fn fetch_organization_name(
    app_state: &AppState,
    organization_id: u64,
) -> Result<String, String> {
    GetOrganizationNameQuery::new(organization_id as i64)
        .execute(app_state)
        .await
        .map_err(|e| format!("Failed to fetch organization name: {}", e))
}

async fn fetch_workspace_name(app_state: &AppState, workspace_id: u64) -> Result<String, String> {
    GetWorkspaceNameQuery::new(workspace_id as i64)
        .execute(app_state)
        .await
        .map_err(|e| format!("Failed to fetch workspace name: {}", e))
}

fn create_verification_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    verification_code: &str,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert("code".to_string(), verification_code.to_string());
    variables.insert("code.expires_in_minutes".to_string(), "15".to_string());

    variables
}

fn create_password_reset_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    reset_code: &str,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert("reset_code".to_string(), reset_code.to_string());
    variables.insert(
        "reset_code.expires_in_minutes".to_string(),
        "15".to_string(),
    );

    variables
}

fn create_signin_notification_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    signin: Option<&SignIn>,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());

    if let Some(signin) = signin {
        variables.insert(
            "signin_time".to_string(),
            signin
                .created_at
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string(),
        );
        variables.insert(
            "device_name".to_string(),
            if signin.device.is_empty() {
                "Unknown Device".to_string()
            } else {
                signin.device.clone()
            },
        );
        variables.insert("browser".to_string(), signin.browser.clone());
        variables.insert("ip_address".to_string(), signin.ip_address.clone());

        let location = if !signin.city.is_empty() && !signin.country.is_empty() {
            format!("{}, {}", signin.city, signin.country)
        } else if !signin.region.is_empty() && !signin.country.is_empty() {
            format!("{}, {}", signin.region, signin.country)
        } else if !signin.country.is_empty() {
            signin.country.clone()
        } else {
            "Unknown Location".to_string()
        };
        variables.insert("location".to_string(), location);
    } else {
        variables.insert("device_name".to_string(), "Unknown Device".to_string());
        variables.insert("location".to_string(), "Unknown Location".to_string());
        variables.insert("browser".to_string(), "Unknown Browser".to_string());
        variables.insert("ip_address".to_string(), "Unknown IP".to_string());
    }

    variables
}

fn create_email_change_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    old_email: &str,
    new_email: &str,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert("old_email".to_string(), old_email.to_string());
    variables.insert("new_email".to_string(), new_email.to_string());

    variables
}

fn create_password_change_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert(
        "change_time".to_string(),
        chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
    );

    variables
}

fn create_password_remove_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert(
        "removal_time".to_string(),
        chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
    );

    variables
}

fn create_waitlist_signup_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert("last_name".to_string(), user.last_name.clone());
    variables.insert(
        "email_address".to_string(),
        user.primary_email_address.clone().unwrap_or_default(),
    );

    variables
}

fn create_waitlist_invite_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    invitation: Option<&DeploymentInvitation>,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert("last_name".to_string(), user.last_name.clone());
    variables.insert(
        "email_address".to_string(),
        user.primary_email_address.clone().unwrap_or_default(),
    );

    if let Some(invitation) = invitation {
        let days_until_expiry = (invitation.expiry - chrono::Utc::now()).num_days();
        variables.insert(
            "invitation.expires_in_days".to_string(),
            days_until_expiry.max(0).to_string(),
        );
        variables.insert(
            "invitation_expiry".to_string(),
            invitation.expiry.format("%Y-%m-%d").to_string(),
        );
    } else {
        variables.insert("invitation.expires_in_days".to_string(), "7".to_string());
    }

    variables
}

fn create_organization_invite_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    organization_name: &str,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert(
        "organization_name".to_string(),
        organization_name.to_string(),
    );
    variables.insert(
        "inviter_name".to_string(),
        format!("{} {}", user.first_name, user.last_name),
    );

    variables
}

fn create_workspace_invite_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    workspace_name: &str,
    invitation: Option<&DeploymentInvitation>,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert("workspace_name".to_string(), workspace_name.to_string());
    variables.insert(
        "inviter_name".to_string(),
        format!("{} {}", user.first_name, user.last_name),
    );

    if let Some(invitation) = invitation {
        let days_until_expiry = (invitation.expiry - chrono::Utc::now()).num_days();
        variables.insert(
            "invitation.expires_in_days".to_string(),
            days_until_expiry.max(0).to_string(),
        );
        variables.insert(
            "invitation_expiry".to_string(),
            invitation.expiry.format("%Y-%m-%d").to_string(),
        );
    }

    variables
}

fn create_magic_link_variables(
    user: &UserDetails,
    deployment: &DeploymentWithSettings,
    magic_link: &str,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();

    let app_name = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.app_name.clone())
        .unwrap_or_else(|| "Your App".to_string());
    let app_logo = deployment
        .ui_settings
        .as_ref()
        .map(|ui| ui.logo_image_url.clone())
        .unwrap_or_else(|| "".to_string());

    variables.insert("app_name".to_string(), app_name);
    variables.insert("app_logo".to_string(), app_logo);
    variables.insert("first_name".to_string(), user.first_name.clone());
    variables.insert("action_url".to_string(), magic_link.to_string());
    variables.insert("link.expires_in_minutes".to_string(), "15".to_string());

    variables
}
