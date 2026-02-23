use common::error::AppError;
use common::state::AppState;
use common::validators::ProjectValidator;
use models::{
    AuthFactorsEnabled, CustomSmtpConfig, DarkModeSettings, Deployment, DeploymentAuthSettings,
    DeploymentB2bSettings, DeploymentB2bSettingsWithRoles, DeploymentEmailTemplate, DeploymentMode,
    DeploymentOrganizationRole, DeploymentRestrictions, DeploymentSmsTemplate,
    DeploymentUISettings, DeploymentWorkspaceRole, EmailProvider, EmailSettings,
    EmailVerificationRecords, FirstFactor, IndividualAuthSettings, LightModeSettings,
    OauthCredentials, PasswordSettings, PhoneSettings, ProjectWithDeployments, SecondFactorPolicy,
    SocialConnectionProvider, UsernameSettings, VerificationPolicy, VerificationStatus
};

use base64::{Engine, engine::general_purpose::STANDARD, prelude::BASE64_STANDARD};
use std::env;
use std::str::FromStr;

use super::Command;

const DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG: &str = "default";

fn generate_signing_secret() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    format!("whsec_{}", STANDARD.encode(bytes))
}

fn generate_nanoid() -> String {
    const EDGE_ALPHABET: [char; 36] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    ];
    const MIDDLE_ALPHABET: [char; 37] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        '-',
    ];
    const LABEL_LEN: usize = 16;
    const MIDDLE_LEN: usize = LABEL_LEN - 2;
    let first = nanoid::nanoid!(1, &EDGE_ALPHABET);
    let middle = nanoid::nanoid!(MIDDLE_LEN, &MIDDLE_ALPHABET);
    let last = nanoid::nanoid!(1, &EDGE_ALPHABET);
    format!("{}{}{}", first, middle, last)
}

pub struct CreateProjectWithStagingDeploymentCommand {
    name: String,
    auth_methods: Vec<String>,
    owner_id: Option<String>,
}

impl CreateProjectWithStagingDeploymentCommand {
    pub fn new(name: String, auth_methods: Vec<String>) -> Self {
        Self {
            name,
            auth_methods,
            owner_id: None,
        }
    }

    pub fn with_owner(mut self, owner_id: String) -> Self {
        self.owner_id = Some(owner_id);
        self
    }

    fn create_b2b_settings(&self, deployment_id: i64) -> DeploymentB2bSettingsWithRoles {
        DeploymentB2bSettingsWithRoles {
            settings: DeploymentB2bSettings {
                deployment_id,
                ..DeploymentB2bSettings::default()
            },
            default_workspace_creator_role: DeploymentWorkspaceRole::admin(),
            default_workspace_member_role: DeploymentWorkspaceRole::member(),
            default_org_creator_role: DeploymentOrganizationRole::admin(),
            default_org_member_role: DeploymentOrganizationRole::member(),
        }
    }

    fn create_auth_settings(&self, deployment_id: i64) -> DeploymentAuthSettings {
        let email_enabled = self.auth_methods.contains(&"email".to_string());
        let phone_enabled = self.auth_methods.contains(&"phone".to_string());
        let username_enabled = self.auth_methods.contains(&"username".to_string());

        let mut first_factor = FirstFactor::EmailPassword;
        let mut alternate_first_factors: Vec<FirstFactor> = Vec::new();

        if email_enabled {
            first_factor = FirstFactor::EmailPassword;
            if phone_enabled {
                alternate_first_factors.push(FirstFactor::PhoneOtp);
            }
            if username_enabled {
                alternate_first_factors.push(FirstFactor::UsernamePassword);
            }
        } else if phone_enabled {
            first_factor = FirstFactor::PhoneOtp;
            if username_enabled {
                alternate_first_factors.push(FirstFactor::UsernamePassword);
            }
        } else if username_enabled {
            first_factor = FirstFactor::UsernamePassword;
        }

        let email_settings = EmailSettings {
            enabled: email_enabled,
            required: email_enabled,
            ..EmailSettings::default()
        };

        let phone_settings = PhoneSettings {
            enabled: phone_enabled,
            required: phone_enabled,
            ..PhoneSettings::default()
        };

        let username_settings = UsernameSettings {
            enabled: username_enabled,
            required: username_enabled,
            ..UsernameSettings::default()
        };

        let password_settings = PasswordSettings::default();
        let first_name_settings = IndividualAuthSettings::default();
        let last_name_settings = IndividualAuthSettings::default();

        let auth_factors_enabled = AuthFactorsEnabled::default()
            .with_email(email_enabled)
            .with_phone(phone_enabled)
            .with_username(username_enabled);

        let verification_policy = VerificationPolicy {
            phone_number: phone_enabled,
            email: email_enabled,
        };

        DeploymentAuthSettings {
            deployment_id,
            email_address: email_settings,
            phone_number: phone_settings,
            username: username_settings,
            first_factor,
            first_name: first_name_settings,
            last_name: last_name_settings,
            password: password_settings,
            auth_factors_enabled,
            verification_policy,
            second_factor_policy: SecondFactorPolicy::None,
            ..DeploymentAuthSettings::default()
        }
    }

    fn create_ui_settings(
        &self,
        deployment_id: i64,
        frontend_host: String,
    ) -> DeploymentUISettings {
        // Ensure frontend_host has https:// protocol
        let frontend_url = if frontend_host.starts_with("https://") {
            frontend_host
        } else {
            format!("https://{}", frontend_host)
        };

        DeploymentUISettings {
            deployment_id,
            app_name: self.name.clone(),
            after_sign_out_all_page_url: format!("{}/sign-in", frontend_url),
            after_sign_out_one_page_url: format!("{}", frontend_url),
            sign_in_page_url: format!("{}/sign-in", frontend_url),
            sign_up_page_url: format!("{}/sign-up", frontend_url),
            dark_mode_settings: DarkModeSettings::default(),
            light_mode_settings: LightModeSettings::default(),
            organization_profile_url: format!("{}/organization", frontend_url),
            create_organization_url: format!("{}/create-organization", frontend_url),
            user_profile_url: format!("{}/me", frontend_url),
            use_initials_for_organization_profile_image: true,
            use_initials_for_user_profile_image: true,
            ..DeploymentUISettings::default()
        }
    }

    fn create_restrictions(&self, deployment_id: i64) -> DeploymentRestrictions {
        DeploymentRestrictions {
            deployment_id,
            allowlist_enabled: false,
            blocklist_enabled: false,
            block_subaddresses: false,
            block_disposable_emails: false,
            block_voip_numbers: false,
            country_restrictions: Default::default(),
            banned_keywords: Default::default(),
            allowlisted_resources: Default::default(),
            blocklisted_resources: Default::default(),
            sign_up_mode: Default::default(),
            waitlist_collect_names: true, // Default to true for backward compatibility
            ..Default::default()
        }
    }

    fn create_sms_templates(&self, deployment_id: i64) -> DeploymentSmsTemplate {
        DeploymentSmsTemplate {
            deployment_id,
            ..Default::default()
        }
    }

    fn create_email_templates(&self, deployment_id: i64) -> DeploymentEmailTemplate {
        DeploymentEmailTemplate {
            deployment_id,
            ..Default::default()
        }
    }
}

impl Command for CreateProjectWithStagingDeploymentCommand {
    type Output = ProjectWithDeployments;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let validator = ProjectValidator::new();
        validator.validate_project_name(&self.name)?;
        validator.validate_auth_methods(&self.auth_methods)?;

        let project_id = app_state.sf.next_id()? as i64;

        let key_pair_task = tokio::task::spawn_blocking(|| {
            // Generate ECDSA keypair for JWT signing (ES256)
            let ecdsa_pair =
                rcgen::KeyPair::generate().map_err(|e| AppError::Internal(e.to_string()))?;

            // Generate RSA keypair for SAML (most IdPs only support RSA for XML signatures)
            let saml_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256).map_err(|e| {
                AppError::Internal(format!("Failed to generate SAML RSA keypair: {}", e))
            })?;

            Ok::<_, AppError>((
                ecdsa_pair.public_key_pem(),
                ecdsa_pair.serialize_pem(),
                saml_pair.public_key_pem(),
                saml_pair.serialize_pem(),
            ))
        });

        let (public_key, private_key, saml_public_key, saml_private_key) =
            key_pair_task.await.map_err(|e| AppError::Internal(e.to_string()))??;

        let mut tx = app_state.db_pool.begin().await?;

        let billing_account = if let Some(ref owner_id) = self.owner_id {
            sqlx::query!(
                "SELECT id, status FROM billing_accounts WHERE owner_id = $1",
                owner_id
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::Validation("No billing account found".to_string()))?
        } else {
            return Err(AppError::Validation(
                "Project must have an owner".to_string(),
            ));
        };

        if billing_account.status != "active" {
            return Err(AppError::Validation(format!(
                "Cannot create project. Billing account status is {}",
                billing_account.status
            )));
        }

        let billing_account_id = billing_account.id;

        let owner_id = self
            .owner_id
            .as_deref()
            .map(|v| v.split("_").last().unwrap());

        let project_row = sqlx::query!(
            r#"
            INSERT INTO projects (id, name, image_url, owner_id, billing_account_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, created_at, updated_at, deleted_at, name, image_url, owner_id, billing_account_id
            "#,
            project_id,
            self.name,
            "",
            owner_id,
            billing_account_id,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let hostname = generate_nanoid();

        let backend_host = format!("{}.frontend-api.services", hostname);
        let frontend_host = format!("{}.trywacht.xyz", hostname);
        let mut publishable_key = String::from("pk_test_");

        let base64_backend_host = BASE64_STANDARD.encode(format!("https://{}", backend_host));
        publishable_key.push_str(&base64_backend_host);

        let deployment_row = sqlx::query!(
            r#"
            INSERT INTO deployments (
                id,
                project_id,
                mode,
                backend_host,
                frontend_host,
                publishable_key,
                maintenance_mode,
                mail_from_host,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, created_at, updated_at, deleted_at,
                     maintenance_mode, backend_host, frontend_host, publishable_key, project_id, mode, mail_from_host
            "#,
            app_state.sf.next_id()? as i64,
            project_row.id,
            "staging",
            backend_host,
            frontend_host,
            publishable_key,
            false,
            "staging.wacht.services",
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let auth_settings = self.create_auth_settings(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_auth_settings (
                id,
                deployment_id,
                email_address,
                phone_number,
                username,
                first_factor,
                first_name,
                last_name,
                password,
                auth_factors_enabled,
                verification_policy,
                second_factor_policy,
                passkey,
                magic_link,
                multi_session_support,
                session_token_lifetime,
                session_validity_period,
                session_inactive_timeout,
                created_at,
                updated_at
            )
            VALUES (
                $1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8,
                $9,
                $10,
                $11,
                $12,
                $13,
                $14,
                $15,
                $16,
                $17,
                $18,
                $19,
                $20
            )
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            serde_json::to_value(&auth_settings.email_address)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.phone_number)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.username)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            auth_settings.first_factor.to_string(),
            serde_json::to_value(&auth_settings.first_name)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.last_name)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.password)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.auth_factors_enabled)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.verification_policy)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            auth_settings.second_factor_policy.to_string(),
            serde_json::to_value(&auth_settings.passkey)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.magic_link)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.multi_session_support)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            auth_settings.session_token_lifetime,
            auth_settings.session_validity_period,
            auth_settings.session_inactive_timeout,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let ui_settings =
            self.create_ui_settings(deployment_row.id, format!("{}.trywacht.xyz", hostname));

        let staging_ui_settings_query = format!(
            r#"
            INSERT INTO deployment_ui_settings (
                id, deployment_id, app_name, tos_page_url, sign_in_page_url, sign_up_page_url,
                after_sign_out_one_page_url, after_sign_out_all_page_url, favicon_image_url,
                logo_image_url, privacy_policy_url, signup_terms_statement, signup_terms_statement_shown,
                light_mode_settings, dark_mode_settings, after_logo_click_url, organization_profile_url,
                create_organization_url, user_profile_url, after_signup_redirect_url, after_signin_redirect_url,
                after_create_organization_redirect_url, use_initials_for_user_profile_image,
                use_initials_for_organization_profile_image, default_user_profile_image_url,
                default_organization_profile_image_url, waitlist_page_url, support_page_url, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30)
            "#
        );

        sqlx::query(&staging_ui_settings_query)
            .bind(app_state.sf.next_id()? as i64)
            .bind(ui_settings.deployment_id)
            .bind(&ui_settings.app_name)
            .bind(&ui_settings.tos_page_url)
            .bind(&ui_settings.sign_in_page_url)
            .bind(&ui_settings.sign_up_page_url)
            .bind(&ui_settings.after_sign_out_one_page_url)
            .bind(&ui_settings.after_sign_out_all_page_url)
            .bind(&ui_settings.favicon_image_url)
            .bind(&ui_settings.logo_image_url)
            .bind(&ui_settings.privacy_policy_url)
            .bind(&ui_settings.signup_terms_statement)
            .bind(ui_settings.signup_terms_statement_shown)
            .bind(
                serde_json::to_value(&ui_settings.light_mode_settings)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .bind(
                serde_json::to_value(&ui_settings.dark_mode_settings)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .bind(&ui_settings.after_logo_click_url)
            .bind(&ui_settings.organization_profile_url)
            .bind(&ui_settings.create_organization_url)
            .bind(&ui_settings.user_profile_url)
            .bind(&ui_settings.after_signup_redirect_url)
            .bind(&ui_settings.after_signin_redirect_url)
            .bind(&ui_settings.after_create_organization_redirect_url)
            .bind(ui_settings.use_initials_for_user_profile_image)
            .bind(ui_settings.use_initials_for_organization_profile_image)
            .bind(&ui_settings.default_user_profile_image_url)
            .bind(&ui_settings.default_organization_profile_image_url)
            .bind(format!("https://{}.trywacht.xyz/waitlist", hostname))
            .bind("")
            .bind(chrono::Utc::now())
            .bind(chrono::Utc::now())
            .execute(&mut *tx)
            .await?;

        let restrictions = self.create_restrictions(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_restrictions (
                id,
                deployment_id,
                allowlist_enabled,
                blocklist_enabled,
                block_subaddresses,
                block_disposable_emails,
                block_voip_numbers,
                country_restrictions,
                banned_keywords,
                allowlisted_resources,
                blocklisted_resources,
                sign_up_mode,
                waitlist_collect_names,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            app_state.sf.next_id()? as i64,
            restrictions.deployment_id,
            restrictions.allowlist_enabled,
            restrictions.blocklist_enabled,
            restrictions.block_subaddresses,
            restrictions.block_disposable_emails,
            restrictions.block_voip_numbers,
            serde_json::to_value(&restrictions.country_restrictions)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            &restrictions.banned_keywords,
            &restrictions.allowlisted_resources,
            &restrictions.blocklisted_resources,
            restrictions.sign_up_mode.to_string(),
            restrictions.waitlist_collect_names,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let mut b2b_settings = self.create_b2b_settings(deployment_row.id);

        let default_workspace_creator_role = sqlx::query!(
            r#"
            INSERT INTO workspace_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)

            RETURNING id
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_workspace_creator_role.name,
            &b2b_settings.default_workspace_creator_role.permissions,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let sms_templates = self.create_sms_templates(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_sms_templates (
                id,
                deployment_id,
                reset_password_code_template,
                verification_code_template,
                password_change_template,
                password_remove_template,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            app_state.sf.next_id()? as i64,
            sms_templates.deployment_id,
            sms_templates.reset_password_code_template,
            sms_templates.verification_code_template,
            sms_templates.password_change_template,
            sms_templates.password_remove_template,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        // Use pre-generated key pair
        sqlx::query!(
            r#"
            INSERT INTO deployment_key_pairs (
                id,
                deployment_id,
                public_key,
                private_key,
                saml_public_key,
                saml_private_key,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            public_key,
            private_key,
            saml_public_key,
            saml_private_key,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let email_templates = self.create_email_templates(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_email_templates (
                id,
                deployment_id,
                organization_invite_template,
                verification_code_template,
                reset_password_code_template,
                primary_email_change_template,
                password_change_template,
                password_remove_template,
                sign_in_from_new_device_template,
                magic_link_template,
                waitlist_signup_template,
                waitlist_invite_template,
                workspace_invite_template,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            app_state.sf.next_id()? as i64,
            email_templates.deployment_id,
            serde_json::to_value(&email_templates.organization_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.verification_code_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.reset_password_code_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.primary_email_change_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.password_change_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.password_remove_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.sign_in_from_new_device_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.magic_link_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.waitlist_signup_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.waitlist_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.workspace_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let default_workspace_member_role = sqlx::query!(
            r#"
            INSERT INTO workspace_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)

            RETURNING id
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_workspace_member_role.name,
            &b2b_settings.default_workspace_member_role.permissions,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let default_org_creator_role = sqlx::query!(
            r#"
            INSERT INTO organization_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)

            RETURNING id
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_org_creator_role.name,
            &b2b_settings.default_org_creator_role.permissions,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let default_org_member_role = sqlx::query!(
            r#"
            INSERT INTO organization_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)

            RETURNING id
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_org_member_role.name,
            &b2b_settings.default_org_member_role.permissions,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        b2b_settings.default_workspace_creator_role.id = default_workspace_creator_role.id;
        b2b_settings.default_workspace_member_role.id = default_workspace_member_role.id;
        b2b_settings.default_org_creator_role.id = default_org_creator_role.id;
        b2b_settings.default_org_member_role.id = default_org_member_role.id;

        sqlx::query!(
            r#"
            INSERT INTO deployment_b2b_settings (
                id,
                deployment_id,
                organizations_enabled,
                workspaces_enabled,
                ip_allowlist_per_org_enabled,
                max_allowed_org_members,
                max_allowed_workspace_members,
                allow_org_deletion,
                allow_workspace_deletion,
                custom_org_role_enabled,
                custom_workspace_role_enabled,
                default_workspace_creator_role_id,
                default_workspace_member_role_id,
                default_org_creator_role_id,
                default_org_member_role_id,
                limit_org_creation_per_user,
                limit_workspace_creation_per_org,
                org_creation_per_user_count,
                workspaces_per_org_count,
                allow_users_to_create_orgs,
                max_orgs_per_user,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.settings.organizations_enabled,
            b2b_settings.settings.workspaces_enabled,
            b2b_settings.settings.ip_allowlist_per_org_enabled,
            b2b_settings.settings.max_allowed_org_members,
            b2b_settings.settings.max_allowed_workspace_members,
            b2b_settings.settings.allow_org_deletion,
            b2b_settings.settings.allow_workspace_deletion,
            b2b_settings.settings.custom_org_role_enabled,
            b2b_settings.settings.custom_workspace_role_enabled,
            b2b_settings.default_workspace_creator_role.id,
            b2b_settings.default_workspace_member_role.id,
            b2b_settings.default_org_creator_role.id,
            b2b_settings.default_org_member_role.id,
            b2b_settings.settings.limit_org_creation_per_user,
            b2b_settings.settings.limit_workspace_creation_per_org,
            b2b_settings.settings.org_creation_per_user_count,
            b2b_settings.settings.workspaces_per_org_count,
            b2b_settings.settings.allow_users_to_create_orgs,
            b2b_settings.settings.max_orgs_per_user,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        // Create social connections for OAuth providers
        let social_providers = [
            "google",
            "apple",
            "facebook",
            "github",
            "microsoft",
            "discord",
            "linkedin",
            "x",
            "gitlab",
        ];

        let empty_credentials = serde_json::to_value(OauthCredentials::default())
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let mut ids = Vec::new();
        let mut deployment_ids = Vec::new();
        let mut providers = Vec::new();
        let mut enableds = Vec::new();
        let mut credentials_list = Vec::new();
        let mut created_ats = Vec::new();
        let mut updated_ats = Vec::new();

        let now = chrono::Utc::now();

        for provider in social_providers.iter() {
            let provider_with_oauth = format!("{}_oauth", provider);
            if (self.auth_methods.contains(&provider.to_string())
                || self.auth_methods.contains(&provider_with_oauth))
                && SocialConnectionProvider::from_str(&provider_with_oauth).is_ok()
            {
                ids.push(app_state.sf.next_id()? as i64);
                deployment_ids.push(deployment_row.id);
                providers.push(provider_with_oauth);
                enableds.push(true);
                credentials_list.push(empty_credentials.clone());
                created_ats.push(now);
                updated_ats.push(now);
            }
        }

        if !ids.is_empty() {
            sqlx::query!(
                r#"
                INSERT INTO deployment_social_connections (
                    id,
                    deployment_id,
                    provider,
                    enabled,
                    credentials,
                    created_at,
                    updated_at
                )
                SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::text[], $4::bool[], $5::jsonb[], $6::timestamptz[], $7::timestamptz[])
                "#,
                &ids,
                &deployment_ids,
                &providers,
                &enableds,
                &credentials_list,
                &created_ats,
                &updated_ats
            )
            .execute(&mut *tx)
            .await?;
        }

        let app_name = deployment_row.id.to_string();
        let console_id = env::var("CONSOLE_DEPLOYMENT_ID")
            .unwrap()
            .parse::<i64>()
            .unwrap();

        sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (deployment_id, app_slug, name, description, is_active, created_at, updated_at, key_prefix)
            VALUES ($1, $2, $3, $4, true, $5, $6, 'sk_')
            "#,
            console_id,
            format!("aa_{}", deployment_row.id),
            app_name,
            format!("API keys for deployment {}", deployment_row.id),
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let signing_secret = generate_signing_secret();
        sqlx::query!(
            r#"
            INSERT INTO webhook_apps (deployment_id, name, description, signing_secret, event_catalog_slug, is_active, created_at, updated_at, app_slug)
            VALUES ($1, $2, $3, $4, $5, true, $6, $7, $8)
            "#,
            console_id,
            app_name,
            format!("Webhooks for deployment {}", deployment_row.id),
            signing_secret,
            DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG,
            chrono::Utc::now(),
            chrono::Utc::now(),
            format!("wh_{}", deployment_row.id)
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO deployment_ai_settings (id, deployment_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let deployment = Deployment {
            id: deployment_row.id,
            created_at: deployment_row.created_at,
            updated_at: deployment_row.updated_at,
            maintenance_mode: deployment_row.maintenance_mode,
            backend_host: deployment_row.backend_host,
            frontend_host: deployment_row.frontend_host,
            publishable_key: deployment_row.publishable_key,
            project_id: deployment_row.project_id,
            mode: DeploymentMode::from(deployment_row.mode),
            mail_from_host: deployment_row.mail_from_host,
            domain_verification_records: None,
            email_verification_records: None,
            email_provider: EmailProvider::default(),
            custom_smtp_config: None,
        };

        Ok(ProjectWithDeployments {
            id: project_row.id,
            image_url: project_row.image_url,
            created_at: project_row.created_at,
            updated_at: project_row.updated_at,
            name: project_row.name,
            owner_id: project_row.owner_id,
            billing_account_id,
            deployments: vec![deployment],
        })
    }
}

pub struct CreateStagingDeploymentCommand {
    project_id: i64,
    auth_methods: Vec<String>,
}

impl CreateStagingDeploymentCommand {
    pub fn new(project_id: i64, auth_methods: Vec<String>) -> Self {
        Self {
            project_id,
            auth_methods,
        }
    }

    fn create_auth_settings(&self, deployment_id: i64) -> DeploymentAuthSettings {
        let email_enabled = self.auth_methods.contains(&"email".to_string());
        let phone_enabled = self.auth_methods.contains(&"phone".to_string());
        let username_enabled = self.auth_methods.contains(&"username".to_string());

        let mut first_factor = FirstFactor::EmailPassword;
        let mut alternate_first_factors: Vec<FirstFactor> = Vec::new();

        if email_enabled {
            first_factor = FirstFactor::EmailPassword;
            if phone_enabled {
                alternate_first_factors.push(FirstFactor::PhoneOtp);
            }
            if username_enabled {
                alternate_first_factors.push(FirstFactor::UsernamePassword);
            }
        } else if phone_enabled {
            first_factor = FirstFactor::PhoneOtp;
            if username_enabled {
                alternate_first_factors.push(FirstFactor::UsernamePassword);
            }
        } else if username_enabled {
            first_factor = FirstFactor::UsernamePassword;
        }

        let email_settings = EmailSettings {
            enabled: email_enabled,
            required: email_enabled,
            ..EmailSettings::default()
        };

        let phone_settings = PhoneSettings {
            enabled: phone_enabled,
            required: phone_enabled,
            ..PhoneSettings::default()
        };

        let username_settings = UsernameSettings {
            enabled: username_enabled,
            required: username_enabled,
            ..UsernameSettings::default()
        };

        let password_settings = PasswordSettings::default();
        let first_name_settings = IndividualAuthSettings::default();
        let last_name_settings = IndividualAuthSettings::default();

        let auth_factors_enabled = AuthFactorsEnabled::default()
            .with_email(email_enabled)
            .with_phone(phone_enabled)
            .with_username(username_enabled);

        let verification_policy = VerificationPolicy {
            phone_number: phone_enabled,
            email: email_enabled,
        };

        DeploymentAuthSettings {
            deployment_id,
            email_address: email_settings,
            phone_number: phone_settings,
            username: username_settings,
            first_factor,
            first_name: first_name_settings,
            last_name: last_name_settings,
            password: password_settings,
            auth_factors_enabled,
            verification_policy,
            second_factor_policy: SecondFactorPolicy::None,
            ..DeploymentAuthSettings::default()
        }
    }

    fn create_b2b_settings(&self, deployment_id: i64) -> DeploymentB2bSettingsWithRoles {
        DeploymentB2bSettingsWithRoles {
            settings: DeploymentB2bSettings {
                deployment_id,
                ..DeploymentB2bSettings::default()
            },
            default_workspace_creator_role: DeploymentWorkspaceRole::admin(),
            default_workspace_member_role: DeploymentWorkspaceRole::member(),
            default_org_creator_role: DeploymentOrganizationRole::admin(),
            default_org_member_role: DeploymentOrganizationRole::member(),
        }
    }

    fn create_ui_settings(&self, deployment_id: i64, app_name: String) -> DeploymentUISettings {
        DeploymentUISettings {
            deployment_id,
            app_name,
            after_sign_out_all_page_url: "https://staging.wacht.services/sign-in".to_string(),
            after_sign_out_one_page_url: "https://staging.wacht.services".to_string(),
            sign_in_page_url: "https://staging.wacht.services/sign-in".to_string(),
            sign_up_page_url: "https://staging.wacht.services/sign-up".to_string(),
            dark_mode_settings: DarkModeSettings::default(),
            light_mode_settings: LightModeSettings::default(),
            organization_profile_url: "https://staging.wacht.services/organization".to_string(),
            create_organization_url: "https://staging.wacht.services/create-organization"
                .to_string(),
            user_profile_url: "https://staging.wacht.services/me".to_string(),
            use_initials_for_organization_profile_image: true,
            use_initials_for_user_profile_image: true,
            ..DeploymentUISettings::default()
        }
    }

    fn create_restrictions(&self, deployment_id: i64) -> DeploymentRestrictions {
        DeploymentRestrictions {
            deployment_id,
            allowlist_enabled: false,
            blocklist_enabled: false,
            block_subaddresses: false,
            block_disposable_emails: false,
            block_voip_numbers: false,
            country_restrictions: Default::default(),
            banned_keywords: Default::default(),
            allowlisted_resources: Default::default(),
            blocklisted_resources: Default::default(),
            sign_up_mode: Default::default(),
            waitlist_collect_names: true,
            ..Default::default()
        }
    }

    fn create_sms_templates(&self, deployment_id: i64) -> DeploymentSmsTemplate {
        DeploymentSmsTemplate {
            deployment_id,
            ..Default::default()
        }
    }

    fn create_email_templates(&self, deployment_id: i64) -> DeploymentEmailTemplate {
        DeploymentEmailTemplate {
            deployment_id,
            ..Default::default()
        }
    }
}

pub struct CreateProductionDeploymentCommand {
    project_id: i64,
    custom_domain: String,
    auth_methods: Vec<String>,
}

impl CreateProductionDeploymentCommand {
    pub fn new(project_id: i64, custom_domain: String, auth_methods: Vec<String>) -> Self {
        Self {
            project_id,
            custom_domain,
            auth_methods,
        }
    }

    fn create_b2b_settings(&self, deployment_id: i64) -> DeploymentB2bSettingsWithRoles {
        DeploymentB2bSettingsWithRoles {
            settings: DeploymentB2bSettings {
                deployment_id,
                ..DeploymentB2bSettings::default()
            },
            default_workspace_creator_role: DeploymentWorkspaceRole::admin(),
            default_workspace_member_role: DeploymentWorkspaceRole::member(),
            default_org_creator_role: DeploymentOrganizationRole::admin(),
            default_org_member_role: DeploymentOrganizationRole::member(),
        }
    }

    fn create_auth_settings(&self, deployment_id: i64) -> DeploymentAuthSettings {
        let email_enabled = self.auth_methods.contains(&"email".to_string());
        let phone_enabled = self.auth_methods.contains(&"phone".to_string());
        let username_enabled = self.auth_methods.contains(&"username".to_string());

        let mut first_factor = FirstFactor::EmailPassword;
        let mut alternate_first_factors: Vec<FirstFactor> = Vec::new();

        if email_enabled {
            first_factor = FirstFactor::EmailPassword;
            if phone_enabled {
                alternate_first_factors.push(FirstFactor::PhoneOtp);
            }
            if username_enabled {
                alternate_first_factors.push(FirstFactor::UsernamePassword);
            }
        } else if phone_enabled {
            first_factor = FirstFactor::PhoneOtp;
            if username_enabled {
                alternate_first_factors.push(FirstFactor::UsernamePassword);
            }
        } else if username_enabled {
            first_factor = FirstFactor::UsernamePassword;
        }

        let email_settings = EmailSettings {
            enabled: email_enabled,
            required: email_enabled,
            ..EmailSettings::default()
        };

        let phone_settings = PhoneSettings {
            enabled: phone_enabled,
            required: phone_enabled,
            ..PhoneSettings::default()
        };

        let username_settings = UsernameSettings {
            enabled: username_enabled,
            required: username_enabled,
            ..UsernameSettings::default()
        };

        let password_settings = PasswordSettings::default();
        let first_name_settings = IndividualAuthSettings::default();
        let last_name_settings = IndividualAuthSettings::default();

        let auth_factors_enabled = AuthFactorsEnabled::default()
            .with_email(email_enabled)
            .with_phone(phone_enabled)
            .with_username(username_enabled);

        let verification_policy = VerificationPolicy {
            phone_number: phone_enabled,
            email: email_enabled,
        };

        DeploymentAuthSettings {
            deployment_id,
            email_address: email_settings,
            phone_number: phone_settings,
            username: username_settings,
            first_factor,
            first_name: first_name_settings,
            last_name: last_name_settings,
            password: password_settings,
            auth_factors_enabled,
            verification_policy,
            second_factor_policy: SecondFactorPolicy::None,
            ..DeploymentAuthSettings::default()
        }
    }

    fn create_ui_settings(
        &self,
        deployment_id: i64,
        frontend_host: String,
        app_name: String,
    ) -> DeploymentUISettings {
        let frontend_url = if frontend_host.starts_with("https://") {
            frontend_host
        } else {
            format!("https://{}", frontend_host)
        };

        DeploymentUISettings {
            deployment_id,
            app_name,
            after_sign_out_all_page_url: format!("{}/sign-in", frontend_url),
            after_sign_out_one_page_url: format!("{}", frontend_url),
            sign_in_page_url: format!("{}/sign-in", frontend_url),
            sign_up_page_url: format!("{}/sign-up", frontend_url),
            dark_mode_settings: DarkModeSettings::default(),
            light_mode_settings: LightModeSettings::default(),
            organization_profile_url: format!("{}/organization", frontend_url),
            create_organization_url: format!("{}/create-organization", frontend_url),
            user_profile_url: format!("{}/me", frontend_url),
            use_initials_for_organization_profile_image: true,
            use_initials_for_user_profile_image: true,
            ..DeploymentUISettings::default()
        }
    }

    fn create_restrictions(&self, deployment_id: i64) -> DeploymentRestrictions {
        DeploymentRestrictions {
            deployment_id,
            allowlist_enabled: false,
            blocklist_enabled: false,
            block_subaddresses: false,
            block_disposable_emails: false,
            block_voip_numbers: false,
            country_restrictions: Default::default(),
            banned_keywords: Default::default(),
            allowlisted_resources: Default::default(),
            blocklisted_resources: Default::default(),
            sign_up_mode: Default::default(),
            waitlist_collect_names: true, // Default to true for backward compatibility
            ..Default::default()
        }
    }

    fn create_sms_templates(&self, deployment_id: i64) -> DeploymentSmsTemplate {
        DeploymentSmsTemplate {
            deployment_id,
            ..Default::default()
        }
    }

    fn create_email_templates(&self, deployment_id: i64) -> DeploymentEmailTemplate {
        DeploymentEmailTemplate {
            deployment_id,
            ..Default::default()
        }
    }

    async fn cleanup_external_resources_on_failure(
        &self,
        app_state: &AppState,
        frontend_hostname: &str,
        backend_hostname: &str,
        domain: &str,
        postmark_domain_id: Option<i64>,
    ) {
        tracing::warn!("Cleaning up external resources for domain: {}", domain);

        if let Err(e) = app_state
            .cloudflare_service
            .delete_custom_hostname(frontend_hostname)
            .await
        {
            tracing::error!(
                "Failed to cleanup frontend hostname {}: {}",
                frontend_hostname,
                e
            );
        } else {
            tracing::info!(
                "Successfully cleaned up frontend hostname: {}",
                frontend_hostname
            );
        }

        if let Err(e) = app_state
            .cloudflare_service
            .delete_custom_hostname(backend_hostname)
            .await
        {
            tracing::error!(
                "Failed to cleanup backend hostname {}: {}",
                backend_hostname,
                e
            );
        } else {
            tracing::info!(
                "Successfully cleaned up backend hostname: {}",
                backend_hostname
            );
        }

        if let Some(domain_id) = postmark_domain_id {
            if let Err(e) = app_state.postmark_service.delete_domain(domain_id).await {
                tracing::error!("Failed to cleanup Postmark domain {}: {}", domain_id, e);
            } else {
                tracing::info!("Successfully cleaned up Postmark domain: {}", domain_id);
            }
        } else {
            tracing::info!("No Postmark domain to cleanup for: {}", domain);
        }
    }
}

impl Command for CreateStagingDeploymentCommand {
    type Output = Deployment;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let validator = ProjectValidator::new();
        validator.validate_auth_methods(&self.auth_methods)?;

        let key_pair_task = tokio::task::spawn_blocking(|| {
            // Generate ECDSA keypair for JWT signing (ES256)
            let ecdsa_pair =
                rcgen::KeyPair::generate().map_err(|e| AppError::Internal(e.to_string()))?;

            // Generate RSA keypair for SAML (most IdPs only support RSA for XML signatures)
            let saml_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256).map_err(|e| {
                AppError::Internal(format!("Failed to generate SAML RSA keypair: {}", e))
            })?;

            Ok::<_, AppError>((
                ecdsa_pair.public_key_pem(),
                ecdsa_pair.serialize_pem(),
                saml_pair.public_key_pem(),
                saml_pair.serialize_pem(),
            ))
        });

        let (public_key, private_key, saml_public_key, saml_private_key) =
            key_pair_task
                .await
                .map_err(|e| AppError::Internal(e.to_string()))??;

        let mut tx = app_state.db_pool.begin().await?;

        let project = sqlx::query!(
            r#"
            SELECT p.name, ba.status
            FROM projects p
            JOIN billing_accounts ba ON p.billing_account_id = ba.id
            WHERE p.id = $1 AND p.deleted_at IS NULL
            "#,
            self.project_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Project with id {} not found", self.project_id))
        })?;

        if project.status != "active" {
            return Err(AppError::Validation(format!(
                "Cannot create deployment. Billing account status is {}",
                project.status
            )));
        }

        // Check staging deployment limit (max 3)
        let staging_count = sqlx::query!(
            "SELECT COUNT(*) as count FROM deployments WHERE project_id = $1 AND mode = 'staging' AND deleted_at IS NULL",
            self.project_id
        )
        .fetch_one(&mut *tx)
        .await?;

        if staging_count.count.unwrap_or(0) >= 3 {
            return Err(AppError::BadRequest(
                "Maximum of 3 staging deployments allowed per project".to_string(),
            ));
        }

        // Generate unique staging hostname
        let backend_host = format!("{}.frontend-api.services", generate_nanoid());
        let frontend_host = backend_host.clone();

        let mut publishable_key = String::from("pk_test_");
        let base64_backend_host = BASE64_STANDARD.encode(format!("https://{}", backend_host));
        publishable_key.push_str(&base64_backend_host);

        let deployment_row = sqlx::query!(
            r#"
            INSERT INTO deployments (
                id,
                project_id,
                mode,
                backend_host,
                frontend_host,
                publishable_key,
                maintenance_mode,
                mail_from_host,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, created_at, updated_at, deleted_at,
                     maintenance_mode, backend_host, frontend_host, publishable_key, project_id, mode, mail_from_host
            "#,
            app_state.sf.next_id()? as i64,
            self.project_id,
            "staging",
            backend_host,
            frontend_host,
            publishable_key,
            false,
            "staging.wacht.services",
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let auth_settings = self.create_auth_settings(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_auth_settings (
                id,
                deployment_id,
                email_address,
                phone_number,
                username,
                first_name,
                last_name,
                password,
                magic_link,
                passkey,
                auth_factors_enabled,
                verification_policy,
                second_factor_policy,
                first_factor,
                multi_session_support,
                session_token_lifetime,
                session_validity_period,
                session_inactive_timeout,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            serde_json::to_value(&auth_settings.email_address)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.phone_number)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.username)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.first_name)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.last_name)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.password)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.magic_link)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.passkey)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.auth_factors_enabled)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&auth_settings.verification_policy)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            auth_settings.second_factor_policy.to_string(),
            auth_settings.first_factor.to_string(),
            serde_json::to_value(&auth_settings.multi_session_support)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            auth_settings.session_token_lifetime,
            auth_settings.session_validity_period,
            auth_settings.session_inactive_timeout,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let ui_settings = self.create_ui_settings(deployment_row.id, project.name.clone());

        sqlx::query!(
            r#"
            INSERT INTO deployment_ui_settings (
                id,
                deployment_id,
                app_name,
                tos_page_url,
                sign_in_page_url,
                sign_up_page_url,
                after_sign_out_one_page_url,
                after_sign_out_all_page_url,
                favicon_image_url,
                logo_image_url,
                privacy_policy_url,
                signup_terms_statement,
                signup_terms_statement_shown,
                light_mode_settings,
                dark_mode_settings,
                after_logo_click_url,
                organization_profile_url,
                create_organization_url,
                default_user_profile_image_url,
                default_organization_profile_image_url,
                user_profile_url,
                use_initials_for_user_profile_image,
                use_initials_for_organization_profile_image,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25)
            "#,
            app_state.sf.next_id()? as i64,
            ui_settings.deployment_id,
            ui_settings.app_name,
            ui_settings.tos_page_url,
            ui_settings.sign_in_page_url,
            ui_settings.sign_up_page_url,
            ui_settings.after_sign_out_one_page_url,
            ui_settings.after_sign_out_all_page_url,
            ui_settings.favicon_image_url,
            ui_settings.logo_image_url,
            ui_settings.privacy_policy_url,
            ui_settings.signup_terms_statement,
            ui_settings.signup_terms_statement_shown,
            serde_json::to_value(&ui_settings.light_mode_settings)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&ui_settings.dark_mode_settings)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            ui_settings.after_logo_click_url,
            ui_settings.organization_profile_url,
            ui_settings.create_organization_url,
            ui_settings.default_user_profile_image_url,
            ui_settings.default_organization_profile_image_url,
            ui_settings.user_profile_url,
            ui_settings.use_initials_for_user_profile_image,
            ui_settings.use_initials_for_organization_profile_image,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let b2b_settings = self.create_b2b_settings(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_b2b_settings (
                id,
                deployment_id,
                default_workspace_creator_role_id,
                default_workspace_member_role_id,
                default_org_creator_role_id,
                default_org_member_role_id,
                limit_org_creation_per_user,
                limit_workspace_creation_per_org,
                org_creation_per_user_count,
                workspaces_per_org_count,
                allow_users_to_create_orgs,
                max_orgs_per_user,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_workspace_creator_role.id,
            b2b_settings.default_workspace_member_role.id,
            b2b_settings.default_org_creator_role.id,
            b2b_settings.default_org_member_role.id,
            b2b_settings.settings.limit_org_creation_per_user,
            b2b_settings.settings.limit_workspace_creation_per_org,
            b2b_settings.settings.org_creation_per_user_count,
            b2b_settings.settings.workspaces_per_org_count,
            b2b_settings.settings.allow_users_to_create_orgs,
            b2b_settings.settings.max_orgs_per_user,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let restrictions = self.create_restrictions(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_restrictions (
                id,
                deployment_id,
                allowlist_enabled,
                blocklist_enabled,
                block_subaddresses,
                block_disposable_emails,
                block_voip_numbers,
                country_restrictions,
                banned_keywords,
                allowlisted_resources,
                blocklisted_resources,
                sign_up_mode,
                waitlist_collect_names,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            restrictions.allowlist_enabled,
            restrictions.blocklist_enabled,
            restrictions.block_subaddresses,
            restrictions.block_disposable_emails,
            restrictions.block_voip_numbers,
            serde_json::to_value(&restrictions.country_restrictions)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            &restrictions.banned_keywords,
            &restrictions.allowlisted_resources,
            &restrictions.blocklisted_resources,
            restrictions.sign_up_mode.to_string(),
            restrictions.waitlist_collect_names,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let sms_templates = self.create_sms_templates(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_sms_templates (
                id,
                deployment_id,
                reset_password_code_template,
                verification_code_template,
                password_change_template,
                password_remove_template,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            app_state.sf.next_id()? as i64,
            sms_templates.deployment_id,
            sms_templates.reset_password_code_template,
            sms_templates.verification_code_template,
            sms_templates.password_change_template,
            sms_templates.password_remove_template,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        // Create empty AI settings row for this deployment
        sqlx::query!(
            r#"
            INSERT INTO deployment_ai_settings (id, deployment_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        // Use pre-generated key pair
        sqlx::query!(
            r#"
            INSERT INTO deployment_key_pairs (
                id,
                deployment_id,
                public_key,
                private_key,
                saml_public_key,
                saml_private_key,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            public_key,
            private_key,
            saml_public_key,
            saml_private_key,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let email_templates = self.create_email_templates(deployment_row.id);

        sqlx::query!(
            r#"
            INSERT INTO deployment_email_templates (
                id,
                deployment_id,
                organization_invite_template,
                verification_code_template,
                reset_password_code_template,
                primary_email_change_template,
                password_change_template,
                password_remove_template,
                sign_in_from_new_device_template,
                magic_link_template,
                waitlist_signup_template,
                waitlist_invite_template,
                workspace_invite_template,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            app_state.sf.next_id()? as i64,
            email_templates.deployment_id,
            serde_json::to_value(&email_templates.organization_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.verification_code_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.reset_password_code_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.primary_email_change_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.password_change_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.password_remove_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.sign_in_from_new_device_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.magic_link_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.waitlist_signup_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.waitlist_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.workspace_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        // Create social connections for OAuth providers
        let social_providers = [
            "google",
            "apple",
            "facebook",
            "github",
            "microsoft",
            "discord",
            "linkedin",
            "x",
            "gitlab",
        ];

        let empty_credentials = serde_json::to_value(OauthCredentials::default())
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let mut ids = Vec::new();
        let mut deployment_ids = Vec::new();
        let mut providers = Vec::new();
        let mut enableds = Vec::new();
        let mut credentials_list = Vec::new();
        let mut created_ats = Vec::new();
        let mut updated_ats = Vec::new();

        let now = chrono::Utc::now();

        for provider in social_providers.iter() {
            let provider_with_oauth = format!("{}_oauth", provider);
            if (self.auth_methods.contains(&provider.to_string())
                || self.auth_methods.contains(&provider_with_oauth))
                && SocialConnectionProvider::from_str(&provider_with_oauth).is_ok()
            {
                ids.push(app_state.sf.next_id()? as i64);
                deployment_ids.push(deployment_row.id);
                providers.push(provider_with_oauth);
                enableds.push(true);
                credentials_list.push(empty_credentials.clone());
                created_ats.push(now);
                updated_ats.push(now);
            }
        }

        if !ids.is_empty() {
            sqlx::query!(
                r#"
                INSERT INTO deployment_social_connections (
                    id,
                    deployment_id,
                    provider,
                    enabled,
                    credentials,
                    created_at,
                    updated_at
                )
                SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::text[], $4::bool[], $5::jsonb[], $6::timestamptz[], $7::timestamptz[])
                "#,
                &ids,
                &deployment_ids,
                &providers,
                &enableds,
                &credentials_list,
                &created_ats,
                &updated_ats
            )
            .execute(&mut *tx)
            .await?;
        }

        let app_name = deployment_row.id.to_string();
        let console_id = env::var("CONSOLE_DEPLOYMENT_ID")
            .unwrap()
            .parse::<i64>()
            .unwrap();

        sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (deployment_id, app_slug, name, description, is_active, created_at, updated_at)
            VALUES ($1, $2, $3, $4, true, $5, $6)
            "#,
            console_id,
            format!("aa_{}", deployment_row.id),
            app_name,
            format!("API keys for deployment {}", deployment_row.id),
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let signing_secret = generate_signing_secret();
        sqlx::query!(
            r#"
            INSERT INTO webhook_apps (deployment_id, name, description, signing_secret, event_catalog_slug, is_active, created_at, updated_at, app_slug)
            VALUES ($1, $2, $3, $4, $5, true, $6, $7, $8)
            "#,
            console_id,
            app_name,
            format!("Webhooks for deployment {}", deployment_row.id),
            signing_secret,
            DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG,
            chrono::Utc::now(),
            chrono::Utc::now(),
            format!("wh_{}", deployment_row.id)
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Deployment {
            id: deployment_row.id,
            created_at: deployment_row.created_at,
            updated_at: deployment_row.updated_at,
            maintenance_mode: deployment_row.maintenance_mode,
            backend_host: deployment_row.backend_host,
            frontend_host: deployment_row.frontend_host,
            publishable_key: deployment_row.publishable_key,
            project_id: deployment_row.project_id,
            mode: DeploymentMode::from(deployment_row.mode),
            mail_from_host: deployment_row.mail_from_host,
            domain_verification_records: None,
            email_verification_records: None,
            email_provider: EmailProvider::default(),
            custom_smtp_config: None,
        })
    }
}

impl Command for CreateProductionDeploymentCommand {
    type Output = Deployment;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let validator = ProjectValidator::new();
        validator.validate_domain_format(&self.custom_domain)?;
        validator.validate_auth_methods(&self.auth_methods)?;

        let key_pair_task = tokio::task::spawn_blocking(|| {
            // Generate ECDSA keypair for JWT signing (ES256)
            let ecdsa_pair =
                rcgen::KeyPair::generate().map_err(|e| AppError::Internal(e.to_string()))?;

            // Generate RSA keypair for SAML (most IdPs only support RSA for XML signatures)
            let saml_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256).map_err(|e| {
                AppError::Internal(format!("Failed to generate SAML RSA keypair: {}", e))
            })?;

            Ok::<_, AppError>((
                ecdsa_pair.public_key_pem(),
                ecdsa_pair.serialize_pem(),
                saml_pair.public_key_pem(),
                saml_pair.serialize_pem(),
            ))
        });

        let (public_key, private_key, saml_public_key, saml_private_key) =
            key_pair_task
                .await
                .map_err(|e| AppError::Internal(e.to_string()))??;

        let mut tx = app_state.db_pool.begin().await?;

        let project = sqlx::query!(
            r#"
            SELECT p.name, ba.status
            FROM projects p
            JOIN billing_accounts ba ON p.billing_account_id = ba.id
            WHERE p.id = $1 AND p.deleted_at IS NULL
            "#,
            self.project_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Project with id {} not found", self.project_id))
        })?;

        if project.status != "active" {
            return Err(AppError::Validation(format!(
                "Cannot create deployment. Billing account status is {}",
                project.status
            )));
        }

        let existing_production = sqlx::query!(
            "SELECT id FROM deployments WHERE project_id = $1 AND mode = 'production' AND deleted_at IS NULL",
            self.project_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if existing_production.is_some() {
            return Err(AppError::BadRequest(
                "A production deployment already exists for this project".to_string(),
            ));
        }

        let existing_domain = sqlx::query!(
            "SELECT id, project_id FROM deployments WHERE (backend_host = $1 OR frontend_host = $2 OR mail_from_host = $3) AND deleted_at IS NULL",
            format!("frontend.{}", self.custom_domain),
            format!("accounts.{}", self.custom_domain),
            self.custom_domain
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(existing) = existing_domain {
            return Err(AppError::BadRequest(format!(
                "Domain '{}' is already in use by another deployment (ID: {})",
                self.custom_domain, existing.id
            )));
        }

        let backend_host = format!("frontend.{}", self.custom_domain);
        let frontend_host = format!("accounts.{}", self.custom_domain);
        let mail_from_host = format!("wcmail.{}", self.custom_domain);

        let domain_verification_records = app_state
            .cloudflare_service
            .generate_domain_verification_records(&frontend_host, &backend_host);

        let empty_email_verification_records = EmailVerificationRecords::default();

        let mut publishable_key = String::from("pk_live_");
        let base64_backend_host = BASE64_STANDARD.encode(format!("https://{}", backend_host));
        publishable_key.push_str(&base64_backend_host);

        let deployment_row = sqlx::query!(
            r#"
            INSERT INTO deployments (
                id,
                project_id,
                mode,
                backend_host,
                frontend_host,
                publishable_key,
                maintenance_mode,
                mail_from_host,
                domain_verification_records,
                email_verification_records,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id, created_at, updated_at, deleted_at,
                     maintenance_mode, backend_host, frontend_host, publishable_key, project_id, mode, mail_from_host,
                     domain_verification_records::jsonb as domain_verification_records,
                     email_verification_records::jsonb as email_verification_records,
                     email_provider, custom_smtp_config::jsonb as custom_smtp_config
            "#,
            app_state.sf.next_id()? as i64,
            self.project_id,
            "production",
            backend_host,
            frontend_host,
            publishable_key,
            false,
            mail_from_host,
            serde_json::to_value(&domain_verification_records)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&empty_email_verification_records)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let _auth_settings = self.create_auth_settings(deployment_row.id);
        let ui_settings = self.create_ui_settings(
            deployment_row.id,
            frontend_host.clone(),
            project.name.clone(),
        );
        let mut b2b_settings = self.create_b2b_settings(deployment_row.id);
        let restrictions = self.create_restrictions(deployment_row.id);
        let email_templates = self.create_email_templates(deployment_row.id);
        let sms_templates = self.create_sms_templates(deployment_row.id);
        // Use pre-generated key pair
        sqlx::query!(
            r#"
            INSERT INTO deployment_key_pairs (
                id,
                deployment_id,
                public_key,
                private_key,
                saml_public_key,
                saml_private_key,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            public_key,
            private_key,
            saml_public_key,
            saml_private_key,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let ui_settings_query = format!(
            r#"
            INSERT INTO deployment_ui_settings (
                id, deployment_id, app_name, tos_page_url, sign_in_page_url, sign_up_page_url,
                after_sign_out_one_page_url, after_sign_out_all_page_url, favicon_image_url,
                logo_image_url, privacy_policy_url, signup_terms_statement, signup_terms_statement_shown,
                light_mode_settings, dark_mode_settings, after_logo_click_url, organization_profile_url,
                create_organization_url, user_profile_url, after_signup_redirect_url, after_signin_redirect_url,
                after_create_organization_redirect_url, use_initials_for_user_profile_image,
                use_initials_for_organization_profile_image, default_user_profile_image_url,
                default_organization_profile_image_url, waitlist_page_url, support_page_url, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30)
            "#
        );

        sqlx::query(&ui_settings_query)
            .bind(app_state.sf.next_id()? as i64)
            .bind(deployment_row.id)
            .bind(&ui_settings.app_name)
            .bind(&ui_settings.tos_page_url)
            .bind(&ui_settings.sign_in_page_url)
            .bind(&ui_settings.sign_up_page_url)
            .bind(&ui_settings.after_sign_out_one_page_url)
            .bind(&ui_settings.after_sign_out_all_page_url)
            .bind(&ui_settings.favicon_image_url)
            .bind(&ui_settings.logo_image_url)
            .bind(&ui_settings.privacy_policy_url)
            .bind(&ui_settings.signup_terms_statement)
            .bind(ui_settings.signup_terms_statement_shown)
            .bind(
                serde_json::to_value(&ui_settings.light_mode_settings)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .bind(
                serde_json::to_value(&ui_settings.dark_mode_settings)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .bind(&ui_settings.after_logo_click_url)
            .bind(&ui_settings.organization_profile_url)
            .bind(&ui_settings.create_organization_url)
            .bind(&ui_settings.user_profile_url)
            .bind(&ui_settings.after_signup_redirect_url)
            .bind(&ui_settings.after_signin_redirect_url)
            .bind(&ui_settings.after_create_organization_redirect_url)
            .bind(ui_settings.use_initials_for_user_profile_image)
            .bind(ui_settings.use_initials_for_organization_profile_image)
            .bind(&ui_settings.default_user_profile_image_url)
            .bind(&ui_settings.default_organization_profile_image_url)
            .bind(format!("{}/waitlist", frontend_host))
            .bind("")
            .bind(chrono::Utc::now())
            .bind(chrono::Utc::now())
            .execute(&mut *tx)
            .await?;

        let default_workspace_creator_role = sqlx::query!(
            r#"
            INSERT INTO workspace_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_workspace_creator_role.name,
            &b2b_settings.default_workspace_creator_role.permissions,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let default_workspace_member_role = sqlx::query!(
            r#"
            INSERT INTO workspace_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_workspace_member_role.name,
            &b2b_settings.default_workspace_member_role.permissions,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let default_org_creator_role = sqlx::query!(
            r#"
            INSERT INTO organization_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_org_creator_role.name,
            &b2b_settings.default_org_creator_role.permissions,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        let default_org_member_role = sqlx::query!(
            r#"
            INSERT INTO organization_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.default_org_member_role.name,
            &b2b_settings.default_org_member_role.permissions,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *tx)
        .await?;

        b2b_settings.default_workspace_creator_role.id = default_workspace_creator_role.id;
        b2b_settings.default_workspace_member_role.id = default_workspace_member_role.id;
        b2b_settings.default_org_creator_role.id = default_org_creator_role.id;
        b2b_settings.default_org_member_role.id = default_org_member_role.id;

        sqlx::query!(
            r#"
            INSERT INTO deployment_b2b_settings (
                id,
                deployment_id,
                organizations_enabled,
                workspaces_enabled,
                ip_allowlist_per_org_enabled,
                max_allowed_org_members,
                max_allowed_workspace_members,
                allow_org_deletion,
                allow_workspace_deletion,
                custom_org_role_enabled,
                custom_workspace_role_enabled,
                default_workspace_creator_role_id,
                default_workspace_member_role_id,
                default_org_creator_role_id,
                default_org_member_role_id,
                limit_org_creation_per_user,
                limit_workspace_creation_per_org,
                org_creation_per_user_count,
                workspaces_per_org_count,
                allow_users_to_create_orgs,
                max_orgs_per_user,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            b2b_settings.settings.organizations_enabled,
            b2b_settings.settings.workspaces_enabled,
            b2b_settings.settings.ip_allowlist_per_org_enabled,
            b2b_settings.settings.max_allowed_org_members,
            b2b_settings.settings.max_allowed_workspace_members,
            b2b_settings.settings.allow_org_deletion,
            b2b_settings.settings.allow_workspace_deletion,
            b2b_settings.settings.custom_org_role_enabled,
            b2b_settings.settings.custom_workspace_role_enabled,
            b2b_settings.default_workspace_creator_role.id,
            b2b_settings.default_workspace_member_role.id,
            b2b_settings.default_org_creator_role.id,
            b2b_settings.default_org_member_role.id,
            b2b_settings.settings.limit_org_creation_per_user,
            b2b_settings.settings.limit_workspace_creation_per_org,
            b2b_settings.settings.org_creation_per_user_count,
            b2b_settings.settings.workspaces_per_org_count,
            b2b_settings.settings.allow_users_to_create_orgs,
            b2b_settings.settings.max_orgs_per_user,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO deployment_restrictions (
                id,
                deployment_id,
                allowlist_enabled,
                blocklist_enabled,
                block_subaddresses,
                block_disposable_emails,
                block_voip_numbers,
                country_restrictions,
                banned_keywords,
                allowlisted_resources,
                blocklisted_resources,
                sign_up_mode,
                waitlist_collect_names,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            restrictions.allowlist_enabled,
            restrictions.blocklist_enabled,
            restrictions.block_subaddresses,
            restrictions.block_disposable_emails,
            restrictions.block_voip_numbers,
            serde_json::to_value(&restrictions.country_restrictions)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            &restrictions.banned_keywords,
            &restrictions.allowlisted_resources,
            &restrictions.blocklisted_resources,
            restrictions.sign_up_mode.to_string(),
            restrictions.waitlist_collect_names,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO deployment_email_templates (
                id,
                deployment_id,
                organization_invite_template,
                verification_code_template,
                reset_password_code_template,
                primary_email_change_template,
                password_change_template,
                password_remove_template,
                sign_in_from_new_device_template,
                magic_link_template,
                waitlist_signup_template,
                waitlist_invite_template,
                workspace_invite_template,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            serde_json::to_value(&email_templates.organization_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.verification_code_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.reset_password_code_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.primary_email_change_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.password_change_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.password_remove_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.sign_in_from_new_device_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.magic_link_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.waitlist_signup_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.waitlist_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_templates.workspace_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO deployment_sms_templates (
                id,
                deployment_id,
                reset_password_code_template,
                verification_code_template,
                password_change_template,
                password_remove_template,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            sms_templates.reset_password_code_template,
            sms_templates.verification_code_template,
            sms_templates.password_change_template,
            sms_templates.password_remove_template,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO deployment_ai_settings (id, deployment_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO deployment_key_pairs (
                id,
                deployment_id,
                public_key,
                private_key,
                saml_public_key,
                saml_private_key,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            app_state.sf.next_id()? as i64,
            deployment_row.id,
            public_key,
            private_key,
            saml_public_key,
            saml_private_key,
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let social_providers = [
            "google",
            "apple",
            "facebook",
            "github",
            "microsoft",
            "discord",
            "linkedin",
            "x",
            "gitlab",
        ];

        let empty_credentials = serde_json::to_value(OauthCredentials::default())
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        for provider in social_providers.iter() {
            let provider_with_oauth = format!("{}_oauth", provider);
            if (self.auth_methods.contains(&provider.to_string())
                || self.auth_methods.contains(&provider_with_oauth))
                && SocialConnectionProvider::from_str(&provider_with_oauth).is_ok()
            {
                sqlx::query!(
                    r#"
                    INSERT INTO deployment_social_connections (
                        id,
                        deployment_id,
                        provider,
                        enabled,
                        credentials,
                        created_at,
                        updated_at
                    )
                    VALUES (
                        $1,
                        $2,
                        $3,
                        true,
                        $4,
                        $5,
                        $6
                    )
                    "#,
                    app_state.sf.next_id()? as i64,
                    deployment_row.id,
                    provider_with_oauth,
                    empty_credentials,
                    chrono::Utc::now(),
                    chrono::Utc::now(),
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        let postmark_domain = app_state
            .postmark_service
            .create_domain(&mail_from_host)
            .await?;
        let postmark_domain_id = postmark_domain.id;
        let email_verification_records = app_state
            .postmark_service
            .generate_email_verification_records(&postmark_domain);

        sqlx::query!(
            r#"
            UPDATE deployments
            SET email_verification_records = $1, updated_at = $2
            WHERE id = $3
            "#,
            serde_json::to_value(&email_verification_records)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            chrono::Utc::now(),
            deployment_row.id
        )
        .execute(&mut *tx)
        .await?;

        let frontend_hostname = format!("accounts.{}", self.custom_domain);
        let backend_hostname = format!("frontend.{}", self.custom_domain);

        let frontend_hostname_result = app_state
            .cloudflare_service
            .create_custom_hostname(&frontend_hostname, "accounts.wacht.services")
            .await;

        let frontend_hostname_id = match frontend_hostname_result {
            Ok(custom_hostname) => {
                tracing::info!(
                    "Successfully created frontend custom hostname: {}",
                    frontend_hostname
                );
                Some(custom_hostname.id)
            }
            Err(e) => {
                tracing::error!("Failed to create frontend custom hostname: {}", e);

                return Err(AppError::External(format!(
                    "Failed to create frontend custom hostname: {}. Deployment has been cleaned up.",
                    e
                )));
            }
        };

        let backend_hostname_result = app_state
            .cloudflare_service
            .create_custom_hostname(&backend_hostname, "frontend.wacht.services")
            .await;

        let backend_hostname_id = match backend_hostname_result {
            Ok(custom_hostname) => {
                tracing::info!(
                    "Successfully created backend custom hostname: {}",
                    backend_hostname
                );
                Some(custom_hostname.id)
            }
            Err(e) => {
                tracing::error!("Failed to create backend custom hostname: {}", e);
                self.cleanup_external_resources_on_failure(
                    app_state,
                    &frontend_hostname,
                    &backend_hostname,
                    &self.custom_domain,
                    Some(postmark_domain_id),
                )
                .await;

                return Err(AppError::External(format!(
                    "Failed to create backend custom hostname: {}. Resources have been cleaned up.",
                    e
                )));
            }
        };

        tracing::info!(
            "Postmark domain created successfully for: {}",
            self.custom_domain
        );

        let mut updated_domain_verification_records = domain_verification_records;
        updated_domain_verification_records.frontend_hostname_id = frontend_hostname_id;
        updated_domain_verification_records.backend_hostname_id = backend_hostname_id;
        sqlx::query!(
            r#"
            UPDATE deployments
            SET domain_verification_records = $1, updated_at = $2
            WHERE id = $3
            "#,
            serde_json::to_value(&updated_domain_verification_records)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            chrono::Utc::now(),
            deployment_row.id
        )
        .execute(&mut *tx)
        .await?;

        let app_name = deployment_row.id.to_string();
        let console_id = env::var("CONSOLE_DEPLOYMENT_ID")
            .unwrap()
            .parse::<i64>()
            .unwrap();

        sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (deployment_id, app_slug, name, description, is_active, created_at, updated_at)
            VALUES ($1, $2, $3, $4, true, $5, $6)
            "#,
            console_id,
            format!("aa_{}", deployment_row.id),
            app_name,
            format!("API keys for deployment {}", deployment_row.id),
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        let signing_secret = generate_signing_secret();
        sqlx::query!(
            r#"
            INSERT INTO webhook_apps (deployment_id, name, description, signing_secret, event_catalog_slug, is_active, created_at, updated_at, app_slug)
            VALUES ($1, $2, $3, $4, $5, true, $6, $7, $8)
            "#,
            console_id,
            app_name,
            format!("Webhooks for deployment {}", deployment_row.id),
            signing_secret,
            DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG,
            chrono::Utc::now(),
            chrono::Utc::now(),
            format!("wh_{}", deployment_row.id)
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        tracing::info!(
            "Successfully created production deployment for domain: {} with hostnames: {}, {}",
            self.custom_domain,
            frontend_hostname,
            backend_hostname
        );

        Ok(Deployment {
            id: deployment_row.id,
            created_at: deployment_row.created_at,
            updated_at: chrono::Utc::now(),
            maintenance_mode: deployment_row.maintenance_mode,
            backend_host: deployment_row.backend_host,
            frontend_host: deployment_row.frontend_host,
            publishable_key: deployment_row.publishable_key,
            project_id: deployment_row.project_id,
            mode: DeploymentMode::from(deployment_row.mode),
            mail_from_host: deployment_row.mail_from_host,
            domain_verification_records: Some(updated_domain_verification_records),
            email_verification_records: Some(email_verification_records),
            email_provider: EmailProvider::from(deployment_row.email_provider),
            custom_smtp_config: deployment_row
                .custom_smtp_config
                .and_then(|v| serde_json::from_value(v).ok())
                .map(|mut c: CustomSmtpConfig| {
                    c.password = String::new();
                    c
                }),
        })
    }
}

pub struct VerifyDeploymentDnsRecordsCommand {
    deployment_id: i64,
}

impl VerifyDeploymentDnsRecordsCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Command for VerifyDeploymentDnsRecordsCommand {
    type Output = Deployment;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get current deployment with DNS records
        let deployment_row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deleted_at,
                   maintenance_mode, backend_host, frontend_host, publishable_key,
                   project_id, mode, mail_from_host,
                   domain_verification_records::jsonb as domain_verification_records,
                   email_verification_records::jsonb as email_verification_records,
                   email_provider, custom_smtp_config::jsonb as custom_smtp_config
            FROM deployments
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        // Extract domain from backend host for email verification
        let domain = if deployment_row.backend_host.starts_with("frontend.") {
            deployment_row
                .backend_host
                .strip_prefix("frontend.")
                .unwrap_or(&deployment_row.backend_host)
        } else {
            &deployment_row.backend_host
        };

        // Get existing records from database or create new ones
        let mut domain_verification_records = deployment_row
            .domain_verification_records
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| {
                app_state
                    .cloudflare_service
                    .generate_domain_verification_records(
                        &deployment_row.frontend_host,
                        &deployment_row.backend_host,
                    )
            });

        let mut email_verification_records = deployment_row
            .email_verification_records
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        // Verify domain records using DNS verification service with Cloudflare integration
        app_state
            .dns_verification_service
            .verify_domain_records(
                &mut domain_verification_records,
                &app_state.cloudflare_service,
            )
            .await
            .map_err(|e| {
                tracing::warn!("Failed to verify domain records: {}", e);
                e
            })
            .unwrap_or(());

        // Verify email records using DNS verification service
        app_state
            .dns_verification_service
            .verify_email_records(&mut email_verification_records)
            .await
            .map_err(|e| {
                tracing::warn!("Failed to verify email records: {}", e);
                e
            })
            .unwrap_or(());

        tracing::info!("DNS verification completed for domain: {}", domain);

        // Determine verification status based on record verification
        let domain_verified = app_state
            .dns_verification_service
            .are_domain_records_verified(&domain_verification_records);

        // Check Postmark email verification status
        let email_verified = app_state
            .dns_verification_service
            .are_email_records_verified(&email_verification_records);

        let verification_status = if domain_verified && email_verified {
            "verified"
        } else {
            "in_progress"
        };

        // Update the deployment with verified records (status update commented out until DB migration)
        sqlx::query!(
            r#"
            UPDATE deployments
            SET domain_verification_records = $1,
                email_verification_records = $2,
                updated_at = $3
            WHERE id = $4
            "#,
            serde_json::to_value(&domain_verification_records)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&email_verification_records)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            chrono::Utc::now(),
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        let _final_verification_status = match verification_status {
            "verified" => VerificationStatus::Verified,
            "in_progress" => VerificationStatus::InProgress,
            _ => VerificationStatus::Pending,
        };

        tracing::info!(
            "DNS verification completed for deployment {}: domain_verified={}, email_verified={}, status={}",
            self.deployment_id,
            domain_verified,
            email_verified,
            verification_status
        );

        Ok(Deployment {
            id: deployment_row.id,
            created_at: deployment_row.created_at,
            updated_at: chrono::Utc::now(),
            maintenance_mode: deployment_row.maintenance_mode,
            backend_host: deployment_row.backend_host,
            frontend_host: deployment_row.frontend_host,
            publishable_key: deployment_row.publishable_key,
            project_id: deployment_row.project_id,
            mode: DeploymentMode::from(deployment_row.mode),
            mail_from_host: deployment_row.mail_from_host,
            domain_verification_records: Some(domain_verification_records),
            email_verification_records: Some(email_verification_records),
            email_provider: EmailProvider::from(deployment_row.email_provider),
            custom_smtp_config: deployment_row
                .custom_smtp_config
                .and_then(|v| serde_json::from_value(v).ok())
                .map(|mut c: CustomSmtpConfig| {
                    c.password = String::new();
                    c
                }),
        })
    }
}

#[allow(dead_code)]
pub struct DeleteProjectCommand {
    id: i64,
    created_by: i64,
}

impl DeleteProjectCommand {
    pub fn new(id: i64, created_by: i64) -> Self {
        Self { id, created_by }
    }
}

impl Command for DeleteProjectCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_pool.begin().await?;

        let deployments = sqlx::query!(
            r#"
            SELECT id FROM deployments
            WHERE project_id = $1 AND deleted_at IS NULL
            "#,
            self.id
        )
        .fetch_all(&mut *tx)
        .await?;

        for deployment in &deployments {
            sqlx::query!(
                r#"
                DELETE FROM deployment_social_connections
                WHERE deployment_id = $1
                "#,
                deployment.id
            )
            .execute(&mut *tx)
            .await?;
        }

        for deployment in &deployments {
            sqlx::query!(
                r#"
                DELETE FROM deployment_auth_settings
                WHERE deployment_id = $1
                "#,
                deployment.id
            )
            .execute(&mut *tx)
            .await?;
        }

        for deployment in &deployments {
            sqlx::query!(
                r#"
                DELETE FROM deployment_ui_settings
                WHERE deployment_id = $1
                "#,
                deployment.id
            )
            .execute(&mut *tx)
            .await?;
        }

        for deployment in &deployments {
            sqlx::query!(
                r#"
                DELETE FROM deployment_b2b_settings
                WHERE deployment_id = $1
                "#,
                deployment.id
            )
            .execute(&mut *tx)
            .await?;
        }

        for deployment in &deployments {
            sqlx::query!(
                r#"
                DELETE FROM deployment_b2b_settings
                WHERE deployment_id = $1
                "#,
                deployment.id
            )
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query!(
            r#"
            DELETE FROM deployments
            WHERE project_id = $1
            "#,
            self.id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            DELETE FROM projects
            WHERE id = $1
            "#,
            self.id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }
}
