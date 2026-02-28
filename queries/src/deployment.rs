use std::str::FromStr;

use common::error::AppError;
use common::state::AppState;
use dto::params::deployment::DeploymentNameParams;
use models::{
    DeploymentAuthSettings, DeploymentB2bSettings, DeploymentB2bSettingsWithRoles,
    DeploymentJwtTemplate, DeploymentMode, DeploymentOrganizationRole, DeploymentRestrictions,
    DeploymentRestrictionsSignUpMode, DeploymentSocialConnection, DeploymentUISettings,
    DeploymentWithSettings, DeploymentWorkspaceRole, EmailTemplate, FirstFactor,
    SecondFactorPolicy,
};
use sqlx::{Row, query};

use super::Query;

pub struct GetDeploymentIdByBackendHostQuery {
    backend_host: String,
}

impl GetDeploymentIdByBackendHostQuery {
    pub fn new(backend_host: String) -> Self {
        Self { backend_host }
    }
}

impl Query for GetDeploymentIdByBackendHostQuery {
    type Output = i64;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = query!(
            "SELECT id FROM deployments WHERE backend_host = $1 AND deleted_at IS NULL",
            self.backend_host
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        let row = row.ok_or_else(|| AppError::NotFound("Deployment not found".to_string()))?;

        Ok(row.id)
    }
}

pub struct GetDeploymentWithKeyPairQuery {
    backend_host: String,
}

impl GetDeploymentWithKeyPairQuery {
    pub fn new(backend_host: String) -> Self {
        Self { backend_host }
    }
}

impl Query for GetDeploymentWithKeyPairQuery {
    type Output = (i64, String); // (deployment_id, public_key)

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = query!(
            r#"
            SELECT d.id, kp.public_key as "public_key?"
            FROM deployments d
            LEFT JOIN deployment_key_pairs kp ON d.id = kp.deployment_id
            WHERE d.backend_host = $1 AND d.deleted_at IS NULL
            "#,
            self.backend_host
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        let row = row.ok_or_else(|| AppError::NotFound("Deployment not found".to_string()))?;

        let public_key = row
            .public_key
            .ok_or_else(|| AppError::NotFound("Deployment key pair not found".to_string()))?;

        Ok((row.id, public_key))
    }
}

pub struct GetDeploymentWithSettingsQuery {
    deployment_id: i64,
}

impl GetDeploymentWithSettingsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentWithSettingsQuery {
    type Output = DeploymentWithSettings;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = query!(
            r#"
            SELECT
                deployments.id,
                deployments.created_at,
                deployments.updated_at,
                deployments.maintenance_mode,
                deployments.backend_host,
                deployments.frontend_host,
                deployments.publishable_key,
                deployments.mode,
                deployments.mail_from_host,
                deployments.domain_verification_records::jsonb as domain_verification_records,
                deployments.email_verification_records::jsonb as email_verification_records,
                deployments.email_provider,
                deployments.custom_smtp_config::jsonb as custom_smtp_config,

                deployment_auth_settings.id as "auth_settings_id?",
                deployment_auth_settings.created_at as "auth_settings_created_at?",
                deployment_auth_settings.updated_at as "auth_settings_updated_at?",
                deployment_auth_settings.email_address::jsonb as "email_address?",
                deployment_auth_settings.phone_number::jsonb as "phone_number?",
                deployment_auth_settings.username::jsonb as username,
                deployment_auth_settings.first_name::jsonb as first_name,
                deployment_auth_settings.last_name::jsonb as last_name,
                deployment_auth_settings.password::jsonb as password,
                deployment_auth_settings.auth_factors_enabled::jsonb as auth_factors_enabled,
                deployment_auth_settings.verification_policy::jsonb as verification_policy,
                deployment_auth_settings.second_factor_policy::text as second_factor_policy,
                deployment_auth_settings.first_factor::text as first_factor,
                deployment_auth_settings.passkey::jsonb as passkey,
                deployment_auth_settings.magic_link::jsonb as magic_link,
                deployment_auth_settings.multi_session_support::jsonb as multi_session_support,
                deployment_auth_settings.session_token_lifetime,
                deployment_auth_settings.session_validity_period,
                deployment_auth_settings.session_inactive_timeout,

                deployment_ui_settings.id as "ui_settings_id?",
                deployment_ui_settings.created_at as "ui_settings_created_at?",
                deployment_ui_settings.updated_at as "ui_settings_updated_at?",
                deployment_ui_settings.app_name,
                deployment_ui_settings.tos_page_url,
                deployment_ui_settings.sign_in_page_url,
                deployment_ui_settings.sign_up_page_url,
                deployment_ui_settings.after_sign_out_one_page_url,
                deployment_ui_settings.after_sign_out_all_page_url,
                deployment_ui_settings.favicon_image_url,
                deployment_ui_settings.logo_image_url,
                deployment_ui_settings.privacy_policy_url,
                deployment_ui_settings.signup_terms_statement,
                deployment_ui_settings.signup_terms_statement_shown,
                deployment_ui_settings.light_mode_settings,
                deployment_ui_settings.dark_mode_settings,
                deployment_ui_settings.after_logo_click_url,
                deployment_ui_settings.organization_profile_url,
                deployment_ui_settings.create_organization_url,
                deployment_ui_settings.default_user_profile_image_url,
                deployment_ui_settings.default_organization_profile_image_url,
                deployment_ui_settings.default_workspace_profile_image_url,
                deployment_ui_settings.use_initials_for_user_profile_image,
                deployment_ui_settings.use_initials_for_organization_profile_image,
                deployment_ui_settings.after_signup_redirect_url,
                deployment_ui_settings.after_signin_redirect_url,
                deployment_ui_settings.user_profile_url,
                deployment_ui_settings.after_create_organization_redirect_url,
                deployment_ui_settings.waitlist_page_url,
                deployment_ui_settings.support_page_url,

                deployment_b2b_settings.id as "b2b_settings_id?",
                deployment_b2b_settings.created_at as "b2b_settings_created_at?",
                deployment_b2b_settings.updated_at as "b2b_settings_updated_at?",
                deployment_b2b_settings.organizations_enabled as "b2b_settings_organizations_enabled?",
                deployment_b2b_settings.workspaces_enabled as "b2b_settings_workspaces_enabled?",
                deployment_b2b_settings.ip_allowlist_per_org_enabled as "b2b_settings_ip_allowlist_per_org_enabled?",
                deployment_b2b_settings.max_allowed_org_members as "b2b_settings_max_allowed_org_members?",
                deployment_b2b_settings.max_allowed_workspace_members as "b2b_settings_max_allowed_workspace_members?",
                deployment_b2b_settings.allow_org_deletion as "b2b_settings_allow_org_deletion?",
                deployment_b2b_settings.allow_workspace_deletion as "b2b_settings_allow_workspace_deletion?",
                deployment_b2b_settings.custom_org_role_enabled as "b2b_settings_custom_org_role_enabled?",
                deployment_b2b_settings.custom_workspace_role_enabled as "b2b_settings_custom_workspace_role_enabled?",
                deployment_b2b_settings.limit_org_creation_per_user as "b2b_settings_limit_org_creation_per_user?",
                deployment_b2b_settings.limit_workspace_creation_per_org as "b2b_settings_limit_workspace_creation_per_org?",
                deployment_b2b_settings.org_creation_per_user_count as "b2b_settings_org_creation_per_user_count?",
                deployment_b2b_settings.workspaces_per_org_count as "b2b_settings_workspaces_per_org_count?",
                deployment_b2b_settings.allow_users_to_create_orgs as "b2b_settings_allow_users_to_create_orgs?",
                deployment_b2b_settings.max_orgs_per_user as "b2b_settings_max_orgs_per_user?",
                deployment_b2b_settings.default_workspace_creator_role_id as "b2b_settings_default_workspace_creator_role_id?",
                deployment_b2b_settings.default_workspace_member_role_id as "b2b_settings_default_workspace_member_role_id?",
                deployment_b2b_settings.default_org_creator_role_id as "b2b_settings_default_org_creator_role_id?",
                deployment_b2b_settings.default_org_member_role_id as "b2b_settings_default_org_member_role_id?",
                deployment_b2b_settings.workspace_permissions as "b2b_settings_workspace_permissions?",
                deployment_b2b_settings.organization_permissions as "b2b_settings_organization_permissions?",
                deployment_b2b_settings.workspace_permission_catalog as "b2b_settings_workspace_permission_catalog?",
                deployment_b2b_settings.organization_permission_catalog as "b2b_settings_organization_permission_catalog?",
                deployment_b2b_settings.ip_allowlist_per_workspace_enabled as "b2b_settings_ip_allowlist_per_workspace_enabled?",
                deployment_b2b_settings.enforce_mfa_per_org_enabled as "b2b_settings_enforce_mfa_per_org_enabled?",
                deployment_b2b_settings.enforce_mfa_per_workspace_enabled as "b2b_settings_enforce_mfa_per_workspace_enabled?",
                deployment_b2b_settings.enterprise_sso_enabled as "b2b_settings_enterprise_sso_enabled?",

                deployment_default_workspace_creator_role.created_at as "default_workspace_creator_role_created_at?",
                deployment_default_workspace_creator_role.updated_at as "default_workspace_creator_role_updated_at?",
                deployment_default_workspace_creator_role.name as "default_workspace_creator_role_name?",
                deployment_default_workspace_creator_role.permissions as "default_workspace_creator_role_permissions?",

                deployment_default_workspace_member_role.created_at as "default_workspace_member_role_created_at?",
                deployment_default_workspace_member_role.updated_at as "default_workspace_member_role_updated_at?",
                deployment_default_workspace_member_role.name as "default_workspace_member_role_name?",
                deployment_default_workspace_member_role.permissions as "default_workspace_member_role_permissions?",

                deployment_default_org_creator_role.created_at as "default_org_creator_role_created_at?",
                deployment_default_org_creator_role.updated_at as "default_org_creator_role_updated_at?",
                deployment_default_org_creator_role.name as "default_org_creator_role_name?",
                deployment_default_org_creator_role.permissions as "default_org_creator_role_permissions?",

                deployment_default_org_member_role.created_at as "default_org_member_role_created_at?",
                deployment_default_org_member_role.updated_at as "default_org_member_role_updated_at?",
                deployment_default_org_member_role.name as "default_org_member_role_name?",
                deployment_default_org_member_role.permissions as "default_org_member_role_permissions?",

                deployment_restrictions.id as "restrictions_id?",
                deployment_restrictions.created_at as "restrictions_created_at?",
                deployment_restrictions.updated_at as "restrictions_updated_at?",
                deployment_restrictions.allowlist_enabled,
                deployment_restrictions.blocklist_enabled,
                deployment_restrictions.block_subaddresses,
                deployment_restrictions.block_disposable_emails,
                deployment_restrictions.block_voip_numbers,
                deployment_restrictions.country_restrictions,
                deployment_restrictions.banned_keywords,
                deployment_restrictions.allowlisted_resources,
                deployment_restrictions.blocklisted_resources,
                deployment_restrictions.sign_up_mode,
                deployment_restrictions.waitlist_collect_names

            FROM deployments
            LEFT JOIN deployment_auth_settings
                ON deployments.id = deployment_auth_settings.deployment_id
            LEFT JOIN deployment_ui_settings
                ON deployments.id = deployment_ui_settings.deployment_id
            LEFT JOIN deployment_restrictions
                ON deployments.id = deployment_restrictions.deployment_id
            LEFT JOIN deployment_b2b_settings
                ON deployments.id = deployment_b2b_settings.deployment_id
            LEFT JOIN workspace_roles AS deployment_default_workspace_creator_role
                ON deployment_default_workspace_creator_role.id = deployment_b2b_settings.default_workspace_creator_role_id
            LEFT JOIN workspace_roles AS deployment_default_workspace_member_role
                ON deployment_default_workspace_member_role.id = deployment_b2b_settings.default_workspace_member_role_id
            LEFT JOIN organization_roles AS deployment_default_org_creator_role
                ON deployment_default_org_creator_role.id = deployment_b2b_settings.default_org_creator_role_id
            LEFT JOIN organization_roles AS deployment_default_org_member_role
                ON deployment_default_org_member_role.id = deployment_b2b_settings.default_org_member_role_id
            WHERE deployments.id = $1 AND deployments.deleted_at IS NULL
            "#,
            self.deployment_id,
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let mode = match row.mode.as_str() {
            "production" => DeploymentMode::Production,
            "staging" => DeploymentMode::Staging,
            _ => {
                return Err(AppError::Database(sqlx::Error::Protocol(format!(
                    "Invalid deployment mode: {}",
                    row.mode
                ))));
            }
        };

        Ok(DeploymentWithSettings {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            maintenance_mode: row.maintenance_mode,
            backend_host: row.backend_host,
            frontend_host: row.frontend_host,
            publishable_key: row.publishable_key,
            mail_from_host: row.mail_from_host,
            mode,
            auth_settings: if row.auth_settings_id.is_some() {
                Some(DeploymentAuthSettings {
                    id: row.auth_settings_id.unwrap(),

                    created_at: row.auth_settings_created_at,
                    updated_at: row.auth_settings_updated_at,
                    email_address: serde_json::from_value(row.email_address.unwrap_or_default())
                        .unwrap(),
                    phone_number: serde_json::from_value(row.phone_number.unwrap_or_default())
                        .unwrap(),
                    username: serde_json::from_value(row.username).unwrap(),
                    first_name: serde_json::from_value(row.first_name).unwrap(),
                    last_name: serde_json::from_value(row.last_name).unwrap(),
                    password: serde_json::from_value(row.password).unwrap(),
                    auth_factors_enabled: serde_json::from_value(row.auth_factors_enabled).unwrap(),
                    verification_policy: serde_json::from_value(row.verification_policy).unwrap(),
                    passkey: serde_json::from_value(row.passkey).unwrap(),
                    magic_link: serde_json::from_value(row.magic_link).unwrap(),
                    second_factor_policy: FromStr::from_str(&row.second_factor_policy).unwrap(),
                    first_factor: FromStr::from_str(&row.first_factor).unwrap(),
                    deployment_id: self.deployment_id,
                    multi_session_support: serde_json::from_value(row.multi_session_support)
                        .unwrap(),
                    session_token_lifetime: row.session_token_lifetime,
                    session_validity_period: row.session_validity_period,
                    session_inactive_timeout: row.session_inactive_timeout,
                })
            } else {
                None
            },
            ui_settings: if row.ui_settings_id.is_some() {
                Some(DeploymentUISettings {
                    id: row.ui_settings_id.unwrap(),
                    created_at: row.ui_settings_created_at,
                    updated_at: row.ui_settings_updated_at,
                    deployment_id: self.deployment_id,
                    app_name: row.app_name,
                    tos_page_url: row.tos_page_url,
                    sign_in_page_url: row.sign_in_page_url,
                    sign_up_page_url: row.sign_up_page_url,
                    after_sign_out_one_page_url: row.after_sign_out_one_page_url,
                    after_sign_out_all_page_url: row.after_sign_out_all_page_url,
                    favicon_image_url: row.favicon_image_url,
                    logo_image_url: row.logo_image_url,
                    privacy_policy_url: row.privacy_policy_url,
                    signup_terms_statement: row.signup_terms_statement,
                    signup_terms_statement_shown: row.signup_terms_statement_shown,
                    light_mode_settings: serde_json::from_value(row.light_mode_settings).unwrap(),
                    dark_mode_settings: serde_json::from_value(row.dark_mode_settings).unwrap(),
                    after_logo_click_url: row.after_logo_click_url,
                    organization_profile_url: row.organization_profile_url,
                    create_organization_url: row.create_organization_url,
                    default_user_profile_image_url: row.default_user_profile_image_url,
                    default_organization_profile_image_url: row
                        .default_organization_profile_image_url,
                    default_workspace_profile_image_url: row.default_workspace_profile_image_url,
                    use_initials_for_user_profile_image: row.use_initials_for_user_profile_image,
                    use_initials_for_organization_profile_image: row
                        .use_initials_for_organization_profile_image,
                    after_signup_redirect_url: row.after_signup_redirect_url,
                    after_signin_redirect_url: row.after_signin_redirect_url,
                    user_profile_url: row.user_profile_url,
                    after_create_organization_redirect_url: row
                        .after_create_organization_redirect_url,
                    waitlist_page_url: row.waitlist_page_url,
                    support_page_url: row.support_page_url,
                })
            } else {
                None
            },
            restrictions: if row.restrictions_id.is_some() {
                Some(DeploymentRestrictions {
                    id: row.restrictions_id.unwrap(),
                    created_at: row.restrictions_created_at,
                    updated_at: row.restrictions_updated_at,
                    deployment_id: self.deployment_id,
                    allowlist_enabled: row.allowlist_enabled,
                    blocklist_enabled: row.blocklist_enabled,
                    block_subaddresses: row.block_subaddresses,
                    block_disposable_emails: row.block_disposable_emails,
                    block_voip_numbers: row.block_voip_numbers,
                    country_restrictions: serde_json::from_value(row.country_restrictions).unwrap(),
                    banned_keywords: row.banned_keywords,
                    allowlisted_resources: row.allowlisted_resources,
                    blocklisted_resources: row.blocklisted_resources,
                    sign_up_mode: DeploymentRestrictionsSignUpMode::from_str(&row.sign_up_mode)
                        .unwrap(),
                    waitlist_collect_names: row.waitlist_collect_names,
                })
            } else {
                None
            },
            b2b_settings: if row.b2b_settings_id.is_some() {
                let b2b_settings = DeploymentB2bSettings {
                    id: row.b2b_settings_id.unwrap(),
                    created_at: row.b2b_settings_created_at.unwrap(),
                    updated_at: row.b2b_settings_updated_at.unwrap(),
                    deployment_id: self.deployment_id,
                    organizations_enabled: row.b2b_settings_organizations_enabled.unwrap(),
                    workspaces_enabled: row.b2b_settings_workspaces_enabled.unwrap(),
                    ip_allowlist_per_org_enabled: row
                        .b2b_settings_ip_allowlist_per_org_enabled
                        .unwrap(),
                    max_allowed_org_members: row.b2b_settings_max_allowed_org_members.unwrap(),
                    max_allowed_workspace_members: row
                        .b2b_settings_max_allowed_workspace_members
                        .unwrap(),
                    allow_org_deletion: row.b2b_settings_allow_org_deletion.unwrap(),
                    allow_workspace_deletion: row.b2b_settings_allow_workspace_deletion.unwrap(),
                    custom_org_role_enabled: row.b2b_settings_custom_org_role_enabled.unwrap(),
                    custom_workspace_role_enabled: row
                        .b2b_settings_custom_workspace_role_enabled
                        .unwrap(),
                    default_workspace_creator_role_id: row
                        .b2b_settings_default_workspace_creator_role_id
                        .unwrap(),
                    default_workspace_member_role_id: row
                        .b2b_settings_default_workspace_member_role_id
                        .unwrap(),
                    default_org_creator_role_id: row
                        .b2b_settings_default_org_creator_role_id
                        .unwrap(),
                    default_org_member_role_id: row
                        .b2b_settings_default_org_member_role_id
                        .unwrap(),
                    limit_org_creation_per_user: row
                        .b2b_settings_limit_org_creation_per_user
                        .unwrap(),
                    limit_workspace_creation_per_org: row
                        .b2b_settings_limit_workspace_creation_per_org
                        .unwrap(),
                    org_creation_per_user_count: row
                        .b2b_settings_org_creation_per_user_count
                        .unwrap(),
                    workspaces_per_org_count: row.b2b_settings_workspaces_per_org_count.unwrap(),
                    allow_users_to_create_orgs: row
                        .b2b_settings_allow_users_to_create_orgs
                        .unwrap(),
                    max_orgs_per_user: row.b2b_settings_max_orgs_per_user.unwrap(),
                    workspace_permissions: row.b2b_settings_workspace_permissions,
                    organization_permissions: row.b2b_settings_organization_permissions,
                    workspace_permission_catalog: row
                        .b2b_settings_workspace_permission_catalog
                        .map(serde_json::from_value)
                        .transpose()
                        .unwrap(),
                    organization_permission_catalog: row
                        .b2b_settings_organization_permission_catalog
                        .map(serde_json::from_value)
                        .transpose()
                        .unwrap(),
                    ip_allowlist_per_workspace_enabled: row
                        .b2b_settings_ip_allowlist_per_workspace_enabled
                        .unwrap(),
                    enforce_mfa_per_org_enabled: row
                        .b2b_settings_enforce_mfa_per_org_enabled
                        .unwrap(),
                    enforce_mfa_per_workspace_enabled: row
                        .b2b_settings_enforce_mfa_per_workspace_enabled
                        .unwrap(),
                    enterprise_sso_enabled: row
                        .b2b_settings_enterprise_sso_enabled
                        .unwrap_or(false),
                };
                Some(DeploymentB2bSettingsWithRoles {
                    settings: b2b_settings,
                    default_workspace_creator_role: DeploymentWorkspaceRole {
                        id: row.b2b_settings_default_workspace_creator_role_id.unwrap(),
                        created_at: row.default_workspace_creator_role_created_at.unwrap(),
                        updated_at: row.default_workspace_creator_role_updated_at.unwrap(),
                        name: row.default_workspace_creator_role_name.unwrap_or_default(),
                        permissions: row
                            .default_workspace_creator_role_permissions
                            .unwrap_or_default(),
                        deployment_id: self.deployment_id,
                        organization_id: None,
                        workspace_id: None,
                    },
                    default_workspace_member_role: DeploymentWorkspaceRole {
                        id: row.b2b_settings_default_workspace_member_role_id.unwrap(),
                        created_at: row.default_workspace_member_role_created_at.unwrap(),
                        updated_at: row.default_workspace_member_role_updated_at.unwrap(),
                        name: row.default_workspace_member_role_name.unwrap_or_default(),
                        permissions: row
                            .default_workspace_member_role_permissions
                            .unwrap_or_default(),
                        deployment_id: self.deployment_id,
                        organization_id: None,
                        workspace_id: None,
                    },
                    default_org_creator_role: DeploymentOrganizationRole {
                        id: row.b2b_settings_default_org_creator_role_id.unwrap(),
                        created_at: row.default_org_creator_role_created_at.unwrap(),
                        updated_at: row.default_org_creator_role_updated_at.unwrap(),
                        name: row.default_org_creator_role_name.unwrap_or_default(),
                        permissions: row.default_org_creator_role_permissions.unwrap_or_default(),
                        deployment_id: self.deployment_id,
                        organization_id: None,
                    },
                    default_org_member_role: DeploymentOrganizationRole {
                        id: row.b2b_settings_default_org_member_role_id.unwrap(),
                        created_at: row.default_org_member_role_created_at.unwrap(),
                        updated_at: row.default_org_member_role_updated_at.unwrap(),
                        name: row.default_org_member_role_name.unwrap_or_default(),
                        permissions: row.default_org_member_role_permissions.unwrap_or_default(),
                        deployment_id: self.deployment_id,
                        organization_id: None,
                    },
                })
            } else {
                None
            },
            domain_verification_records: row
                .domain_verification_records
                .and_then(|v| serde_json::from_value(v).ok()),
            email_verification_records: row
                .email_verification_records
                .and_then(|v| serde_json::from_value(v).ok()),
            email_provider: models::EmailProvider::from(row.email_provider),
            custom_smtp_config: row
                .custom_smtp_config
                .and_then(|v| serde_json::from_value(v).ok())
                .map(|mut c: models::CustomSmtpConfig| {
                    c.password = String::new();
                    c
                }),
        })
    }
}

pub struct GetDeploymentSocialConnectionsQuery {
    deployment_id: i64,
}

impl GetDeploymentSocialConnectionsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentSocialConnectionsQuery {
    type Output = Vec<DeploymentSocialConnection>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = query!(
            r#"
            SELECT
                id,
                created_at,
                updated_at,
                deployment_id,
                provider,
                enabled,
                credentials
            FROM deployment_social_connections
            WHERE deployment_id = $1
            "#,
            self.deployment_id,
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(row
            .into_iter()
            .map(|row| DeploymentSocialConnection {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                deployment_id: row.deployment_id,
                provider: row.provider.map(|s| FromStr::from_str(&s).unwrap()),
                enabled: row.enabled,
                credentials: row
                    .credentials
                    .map(|v| serde_json::from_value(v).unwrap_or_default()),
            })
            .collect())
    }
}

pub struct GetDeploymentJwtTemplatesQuery {
    deployment_id: i64,
}

impl GetDeploymentJwtTemplatesQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentJwtTemplatesQuery {
    type Output = Vec<DeploymentJwtTemplate>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = query!(
            r#"
            SELECT
                id,
                created_at,
                updated_at,
                deployment_id,
                name,
                token_lifetime,
                allowed_clock_skew,
                custom_signing_key,
                template
            FROM deployment_jwt_templates
            WHERE deployment_id = $1
            "#,
            self.deployment_id,
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        let templates = row
            .into_iter()
            .map(|row| DeploymentJwtTemplate {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                deployment_id: row.deployment_id,
                name: row.name,
                token_lifetime: row.token_lifetime,
                allowed_clock_skew: row.allowed_clock_skew,
                custom_signing_key: row
                    .custom_signing_key
                    .map(|v| serde_json::from_value(v).unwrap_or_default()),
                template: row.template,
            })
            .collect();

        Ok(templates)
    }
}

pub struct GetDeploymentEmailTemplateQuery {
    deployment_id: i64,
    template_name: DeploymentNameParams,
}

impl GetDeploymentEmailTemplateQuery {
    pub fn new(deployment_id: i64, template_name: DeploymentNameParams) -> Self {
        Self {
            deployment_id,
            template_name,
        }
    }
}

impl Query for GetDeploymentEmailTemplateQuery {
    type Output = EmailTemplate;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let template = match self.template_name {
            DeploymentNameParams::OrganizationInviteTemplate => {
                let row = query!(
                    r#"
                    SELECT organization_invite_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.organization_invite_template
            }
            DeploymentNameParams::VerificationCodeTemplate => {
                let row = query!(
                    r#"
                    SELECT verification_code_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.verification_code_template
            }
            DeploymentNameParams::ResetPasswordCodeTemplate => {
                let row = query!(
                    r#"
                    SELECT reset_password_code_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.reset_password_code_template
            }
            DeploymentNameParams::PrimaryEmailChangeTemplate => {
                let row = query!(
                    r#"
                    SELECT primary_email_change_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.primary_email_change_template
            }
            DeploymentNameParams::PasswordChangeTemplate => {
                let row = query!(
                    r#"
                    SELECT password_change_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.password_change_template
            }
            DeploymentNameParams::PasswordRemoveTemplate => {
                let row = query!(
                    r#"
                    SELECT password_remove_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.password_remove_template
            }
            DeploymentNameParams::SignInFromNewDeviceTemplate => {
                let row = query!(
                    r#"
                    SELECT sign_in_from_new_device_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.sign_in_from_new_device_template
            }
            DeploymentNameParams::MagicLinkTemplate => {
                let row = query!(
                    r#"
                    SELECT magic_link_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.magic_link_template
            }
            DeploymentNameParams::WaitlistSignupTemplate => {
                let row = query!(
                    r#"
                    SELECT waitlist_signup_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.waitlist_signup_template
            }
            DeploymentNameParams::WaitlistInviteTemplate => {
                let row = query!(
                    r#"
                    SELECT waitlist_invite_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.waitlist_invite_template
            }
            DeploymentNameParams::WorkspaceInviteTemplate => {
                let row = query!(
                    r#"
                    SELECT workspace_invite_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.workspace_invite_template
            }
            DeploymentNameParams::WebhookFailureNotificationTemplate => {
                let row = query!(
                    r#"
                    SELECT webhook_failure_notification_template FROM deployment_email_templates WHERE deployment_id = $1
                    "#,
                    self.deployment_id,
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                row.webhook_failure_notification_template
            }
        };

        Ok(serde_json::from_value(template)?)
    }
}

// Helper query for getting specific email templates by name
pub struct GetEmailTemplateByNameQuery {
    deployment_id: i64,
    template_name: String,
}

impl GetEmailTemplateByNameQuery {
    pub fn new(deployment_id: i64, template_name: String) -> Self {
        Self {
            deployment_id,
            template_name,
        }
    }
}

impl Query for GetEmailTemplateByNameQuery {
    type Output = EmailTemplate;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let column_name = match self.template_name.as_str() {
            "organization_invite_template" => "organization_invite_template",
            "verification_code_template" => "verification_code_template",
            "reset_password_code_template" => "reset_password_code_template",
            "primary_email_change_template" => "primary_email_change_template",
            "password_change_template" => "password_change_template",
            "password_remove_template" => "password_remove_template",
            "sign_in_from_new_device_template" => "sign_in_from_new_device_template",
            "magic_link_template" => "magic_link_template",
            "waitlist_signup_template" => "waitlist_signup_template",
            "waitlist_invite_template" => "waitlist_invite_template",
            "workspace_invite_template" => "workspace_invite_template",
            "webhook_failure_notification_template" => "webhook_failure_notification_template",
            _ => {
                return Err(AppError::BadRequest(
                    "Invalid email template name".to_string(),
                ));
            }
        };

        let query_str = format!(
            "SELECT {} FROM deployment_email_templates WHERE deployment_id = $1",
            column_name
        );

        let row = sqlx::query(&query_str)
            .bind(self.deployment_id)
            .fetch_one(&app_state.db_pool)
            .await?;

        let template_json: serde_json::Value = row.get(column_name);
        let template: EmailTemplate = serde_json::from_value(template_json)
            .map_err(|e| AppError::BadRequest(format!("Failed to parse email template: {}", e)))?;

        Ok(template)
    }
}

// Query for getting deployment auth settings
pub struct GetDeploymentAuthSettingsQuery {
    deployment_id: i64,
}

impl GetDeploymentAuthSettingsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentAuthSettingsQuery {
    type Output = models::DeploymentAuthSettings;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT
                id, created_at, updated_at, deployment_id,
                email_address, phone_number, username, first_name, last_name,
                password, magic_link, passkey, auth_factors_enabled,
                verification_policy, second_factor_policy, first_factor,
                multi_session_support, session_token_lifetime, session_validity_period,
                session_inactive_timeout
            FROM deployment_auth_settings
            WHERE deployment_id = $1
            "#,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let auth_settings = models::DeploymentAuthSettings {
            id: row.id,
            created_at: Some(row.created_at),
            updated_at: Some(row.updated_at),
            deployment_id: row.deployment_id,
            email_address: serde_json::from_value(row.email_address).map_err(|e| {
                AppError::BadRequest(format!("Failed to parse email_address: {}", e))
            })?,
            phone_number: serde_json::from_value(row.phone_number).map_err(|e| {
                AppError::BadRequest(format!("Failed to parse phone_number: {}", e))
            })?,
            username: serde_json::from_value(row.username)
                .map_err(|e| AppError::BadRequest(format!("Failed to parse username: {}", e)))?,
            first_name: serde_json::from_value(row.first_name)
                .map_err(|e| AppError::BadRequest(format!("Failed to parse first_name: {}", e)))?,
            last_name: serde_json::from_value(row.last_name)
                .map_err(|e| AppError::BadRequest(format!("Failed to parse last_name: {}", e)))?,
            password: serde_json::from_value(row.password)
                .map_err(|e| AppError::BadRequest(format!("Failed to parse password: {}", e)))?,
            magic_link: serde_json::from_value(row.magic_link).ok(),
            passkey: serde_json::from_value(row.passkey).ok(),
            auth_factors_enabled: serde_json::from_value(row.auth_factors_enabled).map_err(
                |e| AppError::BadRequest(format!("Failed to parse auth_factors_enabled: {}", e)),
            )?,
            verification_policy: serde_json::from_value(row.verification_policy).map_err(|e| {
                AppError::BadRequest(format!("Failed to parse verification_policy: {}", e))
            })?,
            second_factor_policy: if row.second_factor_policy.trim().is_empty() {
                SecondFactorPolicy::Optional
            } else {
                SecondFactorPolicy::from_str(&row.second_factor_policy).map_err(|e| {
                    AppError::BadRequest(format!(
                        "Failed to parse second_factor_policy '{}': {}",
                        row.second_factor_policy, e
                    ))
                })?
            },
            first_factor: if row.first_factor.trim().is_empty() {
                FirstFactor::EmailPassword
            } else {
                FirstFactor::from_str(&row.first_factor).map_err(|e| {
                    AppError::BadRequest(format!(
                        "Failed to parse first_factor '{}': {}",
                        row.first_factor, e
                    ))
                })?
            },
            multi_session_support: serde_json::from_value(row.multi_session_support).map_err(
                |e| AppError::BadRequest(format!("Failed to parse multi_session_support: {}", e)),
            )?,
            session_token_lifetime: row.session_token_lifetime,
            session_validity_period: row.session_validity_period,
            session_inactive_timeout: row.session_inactive_timeout,
        };

        Ok(auth_settings)
    }
}

// Query to get deployment with its project for access control
pub struct GetDeploymentWithProjectQuery {
    deployment_id: i64,
}

impl GetDeploymentWithProjectQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

pub struct DeploymentWithProject {
    pub deployment_id: i64,
    pub project_id: i64,
    pub project_owner_id: Option<String>,
}

impl Query for GetDeploymentWithProjectQuery {
    type Output = Option<DeploymentWithProject>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT
                d.id as deployment_id,
                d.project_id,
                p.owner_id as project_owner_id
            FROM deployments d
            INNER JOIN projects p ON d.project_id = p.id
            WHERE d.id = $1 AND d.deleted_at IS NULL
            "#,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| DeploymentWithProject {
            deployment_id: r.deployment_id,
            project_id: r.project_id,
            project_owner_id: r.project_owner_id,
        }))
    }
}

pub struct GetDeploymentChargebeeSubscriptionIdQuery {
    pub deployment_id: i64,
}

impl GetDeploymentChargebeeSubscriptionIdQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentChargebeeSubscriptionIdQuery {
    type Output = Option<String>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT s.provider_subscription_id
            FROM deployments d
            JOIN projects p ON d.project_id = p.id
            JOIN billing_accounts ba ON p.billing_account_id = ba.id
            LEFT JOIN subscriptions s ON ba.id = s.billing_account_id
            WHERE d.id = $1
            "#,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.and_then(|r| Some(r.provider_subscription_id)))
    }
}
