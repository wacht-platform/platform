use super::*;
pub(super) struct DeploymentAuthSettingsInsert {
    id: i64,
    auth_settings: DeploymentAuthSettings,
}

#[derive(Default)]
pub(super) struct DeploymentAuthSettingsInsertBuilder {
    id: Option<i64>,
    auth_settings: Option<DeploymentAuthSettings>,
}

impl DeploymentAuthSettingsInsert {
    pub(super) fn builder() -> DeploymentAuthSettingsInsertBuilder {
        DeploymentAuthSettingsInsertBuilder::default()
    }

    pub(super) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

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
            self.id,
            self.auth_settings.deployment_id,
            json_value(&self.auth_settings.email_address)?,
            json_value(&self.auth_settings.phone_number)?,
            json_value(&self.auth_settings.username)?,
            json_value(&self.auth_settings.first_name)?,
            json_value(&self.auth_settings.last_name)?,
            json_value(&self.auth_settings.password)?,
            json_value(&self.auth_settings.magic_link)?,
            json_value(&self.auth_settings.passkey)?,
            json_value(&self.auth_settings.auth_factors_enabled)?,
            json_value(&self.auth_settings.verification_policy)?,
            self.auth_settings.second_factor_policy.to_string(),
            self.auth_settings.first_factor.to_string(),
            json_value(&self.auth_settings.multi_session_support)?,
            self.auth_settings.session_token_lifetime,
            self.auth_settings.session_validity_period,
            self.auth_settings.session_inactive_timeout,
            now,
            now
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentAuthSettingsInsertBuilder {
    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn auth_settings(mut self, auth_settings: DeploymentAuthSettings) -> Self {
        self.auth_settings = Some(auth_settings);
        self
    }

    pub(super) fn build(self) -> Result<DeploymentAuthSettingsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_auth_settings insert id is required".to_string())
        })?;
        let auth_settings = self.auth_settings.ok_or_else(|| {
            AppError::Validation("deployment_auth_settings payload is required".to_string())
        })?;

        Ok(DeploymentAuthSettingsInsert { id, auth_settings })
    }
}

pub(super) struct DeploymentUiSettingsInsert {
    id: i64,
    ui_settings: DeploymentUISettings,
    waitlist_page_url: String,
    support_page_url: String,
}

#[derive(Default)]
pub(super) struct DeploymentUiSettingsInsertBuilder {
    id: Option<i64>,
    ui_settings: Option<DeploymentUISettings>,
    waitlist_page_url: Option<String>,
    support_page_url: Option<String>,
}

impl DeploymentUiSettingsInsert {
    pub(super) fn builder() -> DeploymentUiSettingsInsertBuilder {
        DeploymentUiSettingsInsertBuilder::default()
    }

    pub(super) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

        sqlx::query!(
            "INSERT INTO deployment_ui_settings (id, deployment_id, app_name, tos_page_url, sign_in_page_url, sign_up_page_url, after_sign_out_one_page_url, after_sign_out_all_page_url, favicon_image_url, logo_image_url, privacy_policy_url, signup_terms_statement, signup_terms_statement_shown, light_mode_settings, dark_mode_settings, after_logo_click_url, organization_profile_url, create_organization_url, user_profile_url, after_signup_redirect_url, after_signin_redirect_url, after_create_organization_redirect_url, use_initials_for_user_profile_image, use_initials_for_organization_profile_image, default_user_profile_image_url, default_organization_profile_image_url, waitlist_page_url, support_page_url, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30)",
            self.id,
            self.ui_settings.deployment_id,
            &self.ui_settings.app_name,
            &self.ui_settings.tos_page_url,
            &self.ui_settings.sign_in_page_url,
            &self.ui_settings.sign_up_page_url,
            &self.ui_settings.after_sign_out_one_page_url,
            &self.ui_settings.after_sign_out_all_page_url,
            &self.ui_settings.favicon_image_url,
            &self.ui_settings.logo_image_url,
            &self.ui_settings.privacy_policy_url,
            &self.ui_settings.signup_terms_statement,
            self.ui_settings.signup_terms_statement_shown,
            json_value(&self.ui_settings.light_mode_settings)?,
            json_value(&self.ui_settings.dark_mode_settings)?,
            &self.ui_settings.after_logo_click_url,
            &self.ui_settings.organization_profile_url,
            &self.ui_settings.create_organization_url,
            &self.ui_settings.user_profile_url,
            &self.ui_settings.after_signup_redirect_url,
            &self.ui_settings.after_signin_redirect_url,
            &self.ui_settings.after_create_organization_redirect_url,
            self.ui_settings.use_initials_for_user_profile_image,
            self.ui_settings.use_initials_for_organization_profile_image,
            &self.ui_settings.default_user_profile_image_url,
            &self.ui_settings.default_organization_profile_image_url,
            &self.waitlist_page_url,
            &self.support_page_url,
            now,
            now
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentUiSettingsInsertBuilder {
    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn ui_settings(mut self, ui_settings: DeploymentUISettings) -> Self {
        self.ui_settings = Some(ui_settings);
        self
    }

    pub(super) fn waitlist_page_url(mut self, waitlist_page_url: impl Into<String>) -> Self {
        self.waitlist_page_url = Some(waitlist_page_url.into());
        self
    }

    pub(super) fn support_page_url(mut self, support_page_url: impl Into<String>) -> Self {
        self.support_page_url = Some(support_page_url.into());
        self
    }

    pub(super) fn build(self) -> Result<DeploymentUiSettingsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_ui_settings insert id is required".to_string())
        })?;
        let ui_settings = self.ui_settings.ok_or_else(|| {
            AppError::Validation("deployment_ui_settings payload is required".to_string())
        })?;

        Ok(DeploymentUiSettingsInsert {
            id,
            ui_settings,
            waitlist_page_url: self.waitlist_page_url.unwrap_or_default(),
            support_page_url: self.support_page_url.unwrap_or_default(),
        })
    }
}

pub(super) struct DeploymentRestrictionsInsert {
    id: i64,
    restrictions: DeploymentRestrictions,
}

#[derive(Default)]
pub(super) struct DeploymentRestrictionsInsertBuilder {
    id: Option<i64>,
    restrictions: Option<DeploymentRestrictions>,
}

impl DeploymentRestrictionsInsert {
    pub(super) fn builder() -> DeploymentRestrictionsInsertBuilder {
        DeploymentRestrictionsInsertBuilder::default()
    }

    pub(super) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

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
            self.id,
            self.restrictions.deployment_id,
            self.restrictions.allowlist_enabled,
            self.restrictions.blocklist_enabled,
            self.restrictions.block_subaddresses,
            self.restrictions.block_disposable_emails,
            self.restrictions.block_voip_numbers,
            serde_json::to_value(&self.restrictions.country_restrictions)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            &self.restrictions.banned_keywords,
            &self.restrictions.allowlisted_resources,
            &self.restrictions.blocklisted_resources,
            self.restrictions.sign_up_mode.to_string(),
            self.restrictions.waitlist_collect_names,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentRestrictionsInsertBuilder {
    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn restrictions(mut self, restrictions: DeploymentRestrictions) -> Self {
        self.restrictions = Some(restrictions);
        self
    }

    pub(super) fn build(self) -> Result<DeploymentRestrictionsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_restrictions insert id is required".to_string())
        })?;
        let restrictions = self.restrictions.ok_or_else(|| {
            AppError::Validation("deployment_restrictions payload is required".to_string())
        })?;

        Ok(DeploymentRestrictionsInsert { id, restrictions })
    }
}

pub(super) struct DeploymentSmsTemplatesInsert {
    id: i64,
    sms_templates: DeploymentSmsTemplate,
}

#[derive(Default)]
pub(super) struct DeploymentSmsTemplatesInsertBuilder {
    id: Option<i64>,
    sms_templates: Option<DeploymentSmsTemplate>,
}

impl DeploymentSmsTemplatesInsert {
    pub(super) fn builder() -> DeploymentSmsTemplatesInsertBuilder {
        DeploymentSmsTemplatesInsertBuilder::default()
    }

    pub(super) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

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
            self.id,
            self.sms_templates.deployment_id,
            &self.sms_templates.reset_password_code_template,
            &self.sms_templates.verification_code_template,
            &self.sms_templates.password_change_template,
            &self.sms_templates.password_remove_template,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentSmsTemplatesInsertBuilder {
    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn sms_templates(mut self, sms_templates: DeploymentSmsTemplate) -> Self {
        self.sms_templates = Some(sms_templates);
        self
    }

    pub(super) fn build(self) -> Result<DeploymentSmsTemplatesInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_sms_templates insert id is required".to_string())
        })?;
        let sms_templates = self.sms_templates.ok_or_else(|| {
            AppError::Validation("deployment_sms_templates payload is required".to_string())
        })?;

        Ok(DeploymentSmsTemplatesInsert { id, sms_templates })
    }
}

pub(super) struct DeploymentEmailTemplatesInsert {
    id: i64,
    email_templates: DeploymentEmailTemplate,
}

#[derive(Default)]
pub(super) struct DeploymentEmailTemplatesInsertBuilder {
    id: Option<i64>,
    email_templates: Option<DeploymentEmailTemplate>,
}

impl DeploymentEmailTemplatesInsert {
    pub(super) fn builder() -> DeploymentEmailTemplatesInsertBuilder {
        DeploymentEmailTemplatesInsertBuilder::default()
    }

    pub(super) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

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
            self.id,
            self.email_templates.deployment_id,
            serde_json::to_value(&self.email_templates.organization_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.verification_code_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.reset_password_code_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.primary_email_change_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.password_change_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.password_remove_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.sign_in_from_new_device_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.magic_link_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.waitlist_signup_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.waitlist_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            serde_json::to_value(&self.email_templates.workspace_invite_template)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentEmailTemplatesInsertBuilder {
    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn email_templates(mut self, email_templates: DeploymentEmailTemplate) -> Self {
        self.email_templates = Some(email_templates);
        self
    }

    pub(super) fn build(self) -> Result<DeploymentEmailTemplatesInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_email_templates insert id is required".to_string())
        })?;
        let email_templates = self.email_templates.ok_or_else(|| {
            AppError::Validation("deployment_email_templates payload is required".to_string())
        })?;

        Ok(DeploymentEmailTemplatesInsert {
            id,
            email_templates,
        })
    }
}

pub(super) struct DeploymentAiSettingsInsert {
    id: i64,
    deployment_id: i64,
}

#[derive(Default)]
pub(super) struct DeploymentAiSettingsInsertBuilder {
    id: Option<i64>,
    deployment_id: Option<i64>,
}

impl DeploymentAiSettingsInsert {
    pub(super) fn builder() -> DeploymentAiSettingsInsertBuilder {
        DeploymentAiSettingsInsertBuilder::default()
    }

    pub(super) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO deployment_ai_settings (id, deployment_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4)
            "#,
            self.id,
            self.deployment_id,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentAiSettingsInsertBuilder {
    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(super) fn build(self) -> Result<DeploymentAiSettingsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_ai_settings insert id is required".to_string())
        })?;
        let deployment_id = self.deployment_id.ok_or_else(|| {
            AppError::Validation("deployment_ai_settings deployment_id is required".to_string())
        })?;

        Ok(DeploymentAiSettingsInsert { id, deployment_id })
    }
}

pub(super) struct DeploymentKeyPairsInsert {
    id: i64,
    deployment_id: i64,
    public_key: String,
    private_key: String,
    saml_public_key: String,
    saml_private_key: String,
}

#[derive(Default)]
pub(super) struct DeploymentKeyPairsInsertBuilder {
    id: Option<i64>,
    deployment_id: Option<i64>,
    public_key: Option<String>,
    private_key: Option<String>,
    saml_public_key: Option<String>,
    saml_private_key: Option<String>,
}

impl DeploymentKeyPairsInsert {
    pub(super) fn builder() -> DeploymentKeyPairsInsertBuilder {
        DeploymentKeyPairsInsertBuilder::default()
    }

    pub(super) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

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
            self.id,
            self.deployment_id,
            &self.public_key,
            &self.private_key,
            &self.saml_public_key,
            &self.saml_private_key,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentKeyPairsInsertBuilder {
    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(super) fn public_key(mut self, public_key: String) -> Self {
        self.public_key = Some(public_key);
        self
    }

    pub(super) fn private_key(mut self, private_key: String) -> Self {
        self.private_key = Some(private_key);
        self
    }

    pub(super) fn saml_public_key(mut self, saml_public_key: String) -> Self {
        self.saml_public_key = Some(saml_public_key);
        self
    }

    pub(super) fn saml_private_key(mut self, saml_private_key: String) -> Self {
        self.saml_private_key = Some(saml_private_key);
        self
    }

    pub(super) fn build(self) -> Result<DeploymentKeyPairsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs insert id is required".to_string())
        })?;
        let deployment_id = self.deployment_id.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs deployment_id is required".to_string())
        })?;
        let public_key = self.public_key.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs public_key is required".to_string())
        })?;
        let private_key = self.private_key.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs private_key is required".to_string())
        })?;
        let saml_public_key = self.saml_public_key.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs saml_public_key is required".to_string())
        })?;
        let saml_private_key = self.saml_private_key.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs saml_private_key is required".to_string())
        })?;

        Ok(DeploymentKeyPairsInsert {
            id,
            deployment_id,
            public_key,
            private_key,
            saml_public_key,
            saml_private_key,
        })
    }
}

pub(super) struct DeploymentB2bBootstrapInsert {
    settings_row_id: i64,
    workspace_creator_role_id: i64,
    workspace_member_role_id: i64,
    org_creator_role_id: i64,
    org_member_role_id: i64,
    b2b_settings: DeploymentB2bSettingsWithRoles,
}

#[derive(Default)]
pub(super) struct DeploymentB2bBootstrapInsertBuilder {
    settings_row_id: Option<i64>,
    workspace_creator_role_id: Option<i64>,
    workspace_member_role_id: Option<i64>,
    org_creator_role_id: Option<i64>,
    org_member_role_id: Option<i64>,
    b2b_settings: Option<DeploymentB2bSettingsWithRoles>,
}

impl DeploymentB2bBootstrapInsert {
    pub(super) fn builder() -> DeploymentB2bBootstrapInsertBuilder {
        DeploymentB2bBootstrapInsertBuilder::default()
    }

    #[allow(dead_code)]
    pub(super) async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        self.execute_with_deps(tx.as_mut()).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn execute_with_deps(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now();
        let deployment_id = self.b2b_settings.settings.deployment_id;

        self.insert_workspace_role(
            conn,
            self.workspace_creator_role_id,
            deployment_id,
            &self.b2b_settings.default_workspace_creator_role,
            now,
        )
        .await?;
        self.insert_workspace_role(
            conn,
            self.workspace_member_role_id,
            deployment_id,
            &self.b2b_settings.default_workspace_member_role,
            now,
        )
        .await?;
        self.insert_organization_role(
            conn,
            self.org_creator_role_id,
            deployment_id,
            &self.b2b_settings.default_org_creator_role,
            now,
        )
        .await?;
        self.insert_organization_role(
            conn,
            self.org_member_role_id,
            deployment_id,
            &self.b2b_settings.default_org_member_role,
            now,
        )
        .await?;

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
            self.settings_row_id,
            deployment_id,
            self.b2b_settings.settings.organizations_enabled,
            self.b2b_settings.settings.workspaces_enabled,
            self.b2b_settings.settings.ip_allowlist_per_org_enabled,
            self.b2b_settings.settings.max_allowed_org_members,
            self.b2b_settings.settings.max_allowed_workspace_members,
            self.b2b_settings.settings.allow_org_deletion,
            self.b2b_settings.settings.allow_workspace_deletion,
            self.b2b_settings.settings.custom_org_role_enabled,
            self.b2b_settings.settings.custom_workspace_role_enabled,
            self.workspace_creator_role_id,
            self.workspace_member_role_id,
            self.org_creator_role_id,
            self.org_member_role_id,
            self.b2b_settings.settings.limit_org_creation_per_user,
            self.b2b_settings.settings.limit_workspace_creation_per_org,
            self.b2b_settings.settings.org_creation_per_user_count,
            self.b2b_settings.settings.workspaces_per_org_count,
            self.b2b_settings.settings.allow_users_to_create_orgs,
            self.b2b_settings.settings.max_orgs_per_user,
            now,
            now,
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }

    async fn insert_workspace_role(
        &self,
        conn: &mut sqlx::PgConnection,
        id: i64,
        deployment_id: i64,
        role: &DeploymentWorkspaceRole,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AppError> {
        sqlx::query!(
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
            "#,
            id,
            deployment_id,
            &role.name,
            &role.permissions,
            now,
            now,
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }

    async fn insert_organization_role(
        &self,
        conn: &mut sqlx::PgConnection,
        id: i64,
        deployment_id: i64,
        role: &DeploymentOrganizationRole,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AppError> {
        sqlx::query!(
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
            "#,
            id,
            deployment_id,
            &role.name,
            &role.permissions,
            now,
            now,
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

impl DeploymentB2bBootstrapInsertBuilder {
    pub(super) fn settings_row_id(mut self, settings_row_id: i64) -> Self {
        self.settings_row_id = Some(settings_row_id);
        self
    }

    pub(super) fn workspace_creator_role_id(mut self, workspace_creator_role_id: i64) -> Self {
        self.workspace_creator_role_id = Some(workspace_creator_role_id);
        self
    }

    pub(super) fn workspace_member_role_id(mut self, workspace_member_role_id: i64) -> Self {
        self.workspace_member_role_id = Some(workspace_member_role_id);
        self
    }

    pub(super) fn org_creator_role_id(mut self, org_creator_role_id: i64) -> Self {
        self.org_creator_role_id = Some(org_creator_role_id);
        self
    }

    pub(super) fn org_member_role_id(mut self, org_member_role_id: i64) -> Self {
        self.org_member_role_id = Some(org_member_role_id);
        self
    }

    pub(super) fn b2b_settings(mut self, b2b_settings: DeploymentB2bSettingsWithRoles) -> Self {
        self.b2b_settings = Some(b2b_settings);
        self
    }

    pub(super) fn build(self) -> Result<DeploymentB2bBootstrapInsert, AppError> {
        let settings_row_id = self.settings_row_id.ok_or_else(|| {
            AppError::Validation("deployment_b2b_settings insert id is required".to_string())
        })?;
        let workspace_creator_role_id = self.workspace_creator_role_id.ok_or_else(|| {
            AppError::Validation("workspace creator role id is required".to_string())
        })?;
        let workspace_member_role_id = self.workspace_member_role_id.ok_or_else(|| {
            AppError::Validation("workspace member role id is required".to_string())
        })?;
        let org_creator_role_id = self.org_creator_role_id.ok_or_else(|| {
            AppError::Validation("organization creator role id is required".to_string())
        })?;
        let org_member_role_id = self.org_member_role_id.ok_or_else(|| {
            AppError::Validation("organization member role id is required".to_string())
        })?;
        let b2b_settings = self.b2b_settings.ok_or_else(|| {
            AppError::Validation("deployment_b2b_settings payload is required".to_string())
        })?;

        Ok(DeploymentB2bBootstrapInsert {
            settings_row_id,
            workspace_creator_role_id,
            workspace_member_role_id,
            org_creator_role_id,
            org_member_role_id,
            b2b_settings,
        })
    }
}

pub(super) struct ConsoleAppBootstrapInsert {
    console_deployment_id: i64,
    target_deployment_id: i64,
    event_catalog_slug: String,
}

#[derive(Default)]
pub(super) struct ConsoleAppBootstrapInsertBuilder {
    console_deployment_id: Option<i64>,
    target_deployment_id: Option<i64>,
    event_catalog_slug: Option<String>,
}

impl ConsoleAppBootstrapInsert {
    pub(super) fn builder() -> ConsoleAppBootstrapInsertBuilder {
        ConsoleAppBootstrapInsertBuilder::default()
    }

    #[allow(dead_code)]
    pub(super) async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        self.execute_with_deps(tx.as_mut()).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn execute_with_deps(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let app_name = self.target_deployment_id.to_string();
        let now = chrono::Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (deployment_id, app_slug, name, description, is_active, created_at, updated_at, key_prefix)
            VALUES ($1, $2, $3, $4, true, $5, $6, 'sk_')
            "#,
            self.console_deployment_id,
            format!("aa_{}", self.target_deployment_id),
            &app_name,
            format!("API keys for deployment {}", self.target_deployment_id),
            now,
            now,
        )
        .execute(&mut *conn)
        .await?;

        let signing_secret = generate_signing_secret();

        sqlx::query!(
            r#"
            INSERT INTO webhook_apps (deployment_id, name, description, signing_secret, event_catalog_slug, is_active, created_at, updated_at, app_slug)
            VALUES ($1, $2, $3, $4, $5, true, $6, $7, $8)
            "#,
            self.console_deployment_id,
            &app_name,
            format!("Webhooks for deployment {}", self.target_deployment_id),
            signing_secret,
            &self.event_catalog_slug,
            now,
            now,
            format!("wh_{}", self.target_deployment_id)
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

impl ConsoleAppBootstrapInsertBuilder {
    pub(super) fn console_deployment_id(mut self, console_deployment_id: i64) -> Self {
        self.console_deployment_id = Some(console_deployment_id);
        self
    }

    pub(super) fn target_deployment_id(mut self, target_deployment_id: i64) -> Self {
        self.target_deployment_id = Some(target_deployment_id);
        self
    }

    pub(super) fn event_catalog_slug(mut self, event_catalog_slug: impl Into<String>) -> Self {
        self.event_catalog_slug = Some(event_catalog_slug.into());
        self
    }

    pub(super) fn build(self) -> Result<ConsoleAppBootstrapInsert, AppError> {
        let console_deployment_id = self
            .console_deployment_id
            .ok_or_else(|| AppError::Validation("console deployment id is required".to_string()))?;
        let target_deployment_id = self
            .target_deployment_id
            .ok_or_else(|| AppError::Validation("target deployment id is required".to_string()))?;

        Ok(ConsoleAppBootstrapInsert {
            console_deployment_id,
            target_deployment_id,
            event_catalog_slug: self
                .event_catalog_slug
                .unwrap_or_else(|| DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG.to_string()),
        })
    }
}

pub(super) struct DeploymentSocialConnectionsBulkInsert {
    ids: Vec<i64>,
    deployment_ids: Vec<i64>,
    providers: Vec<String>,
    enableds: Vec<bool>,
    credentials_list: Vec<serde_json::Value>,
    created_ats: Vec<chrono::DateTime<chrono::Utc>>,
    updated_ats: Vec<chrono::DateTime<chrono::Utc>>,
}

impl DeploymentSocialConnectionsBulkInsert {
    pub(super) fn from_auth_methods<F>(
        deployment_id: i64,
        auth_methods: &[String],
        mut next_id: F,
    ) -> Result<Option<Self>, AppError>
    where
        F: FnMut() -> Result<i64, AppError>,
    {
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

        let mut ids = Vec::new();
        let mut deployment_ids = Vec::new();
        let mut providers = Vec::new();
        let mut enableds = Vec::new();
        let mut credentials_list = Vec::new();
        let mut created_ats = Vec::new();
        let mut updated_ats = Vec::new();

        let now = chrono::Utc::now();

        for provider_name in social_providers {
            let provider_with_oauth = format!("{}_oauth", provider_name);
            let is_selected = auth_methods.iter().any(|method| method == provider_name)
                || auth_methods
                    .iter()
                    .any(|method| method == &provider_with_oauth);
            if !is_selected {
                continue;
            }

            if let Ok(provider) = SocialConnectionProvider::from_str(&provider_with_oauth) {
                ids.push(next_id()?);
                deployment_ids.push(deployment_id);
                providers.push(provider_with_oauth);
                enableds.push(true);
                credentials_list.push(social_credentials_with_default_scopes(&provider)?);
                created_ats.push(now);
                updated_ats.push(now);
            }
        }

        if ids.is_empty() {
            return Ok(None);
        }

        Ok(Some(Self {
            ids,
            deployment_ids,
            providers,
            enableds,
            credentials_list,
            created_ats,
            updated_ats,
        }))
    }

    pub(super) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
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
                SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::text[], $4::bool[], $5::jsonb[], $6::timestamptz[], $7::timestamptz[])
                "#,
            &self.ids,
            &self.deployment_ids,
            &self.providers,
            &self.enableds,
            &self.credentials_list,
            &self.created_ats,
            &self.updated_ats
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub(super) struct BillingAccountLockResult {
    pub(super) id: i64,
    pub(super) status: String,
    pub(super) pulse_usage_disabled: bool,
}

#[derive(Default)]
pub(super) struct BillingAccountForOwnerLockQuery {
    owner_id: Option<String>,
}

impl BillingAccountForOwnerLockQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn owner_id(mut self, owner_id: impl Into<String>) -> Self {
        self.owner_id = Some(owner_id.into());
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<Option<BillingAccountLockResult>, AppError> {
        let owner_id = self
            .owner_id
            .as_deref()
            .ok_or_else(|| AppError::Validation("owner_id is required".to_string()))?;

        let row = sqlx::query!(
            "SELECT id, status, COALESCE(pulse_usage_disabled, false) AS \"pulse_usage_disabled!\" FROM billing_accounts WHERE owner_id = $1 FOR UPDATE",
            owner_id
        )
        .fetch_optional(conn)
        .await?;

        Ok(row.map(|r| BillingAccountLockResult {
            id: r.id,
            status: r.status,
            pulse_usage_disabled: r.pulse_usage_disabled,
        }))
    }
}

#[derive(Default)]
pub(super) struct ProjectsCountByBillingAccountQuery {
    billing_account_id: Option<i64>,
}

impl ProjectsCountByBillingAccountQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn billing_account_id(mut self, billing_account_id: i64) -> Self {
        self.billing_account_id = Some(billing_account_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<i64, AppError> {
        let billing_account_id = self
            .billing_account_id
            .ok_or_else(|| AppError::Validation("billing_account_id is required".to_string()))?;

        let row = sqlx::query!(
            r#"
            SELECT COUNT(*)::BIGINT as "count!"
            FROM projects
            WHERE billing_account_id = $1
              AND deleted_at IS NULL
            "#,
            billing_account_id
        )
        .fetch_one(conn)
        .await?;

        Ok(row.count)
    }
}

pub(super) struct ProjectInsertedRow {
    pub(super) id: i64,
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
    pub(super) updated_at: chrono::DateTime<chrono::Utc>,
    pub(super) name: String,
    pub(super) image_url: String,
    pub(super) owner_id: Option<String>,
}

#[derive(Default)]
pub(super) struct ProjectInsert {
    id: Option<i64>,
    name: Option<String>,
    owner_id_fragment: Option<String>,
    billing_account_id: Option<i64>,
}

impl ProjectInsert {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub(super) fn owner_id_fragment(mut self, owner_id_fragment: impl Into<String>) -> Self {
        self.owner_id_fragment = Some(owner_id_fragment.into());
        self
    }

    pub(super) fn billing_account_id(mut self, billing_account_id: i64) -> Self {
        self.billing_account_id = Some(billing_account_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<ProjectInsertedRow, AppError> {
        let id = self
            .id
            .ok_or_else(|| AppError::Validation("project id is required".to_string()))?;
        let name = self
            .name
            .as_deref()
            .ok_or_else(|| AppError::Validation("project name is required".to_string()))?;
        let owner_id_fragment = self.owner_id_fragment.as_deref().ok_or_else(|| {
            AppError::Validation("project owner id fragment is required".to_string())
        })?;
        let billing_account_id = self.billing_account_id.ok_or_else(|| {
            AppError::Validation("project billing_account_id is required".to_string())
        })?;

        let now = chrono::Utc::now();

        let row = sqlx::query!(
            r#"
            INSERT INTO projects (id, name, image_url, owner_id, billing_account_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, created_at, updated_at, name, image_url, owner_id
            "#,
            id,
            name,
            "",
            Some(owner_id_fragment),
            billing_account_id,
            now,
            now,
        )
        .fetch_one(conn)
        .await?;

        Ok(ProjectInsertedRow {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            name: row.name,
            image_url: row.image_url,
            owner_id: row.owner_id,
        })
    }
}

pub(super) struct StagingDeploymentInsertedRow {
    pub(super) id: i64,
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
    pub(super) updated_at: chrono::DateTime<chrono::Utc>,
    pub(super) maintenance_mode: bool,
    pub(super) backend_host: String,
    pub(super) frontend_host: String,
    pub(super) publishable_key: String,
    pub(super) project_id: i64,
    pub(super) mode: String,
    pub(super) mail_from_host: String,
}

#[derive(Default)]
pub(super) struct StagingDeploymentInsert {
    id: Option<i64>,
    project_id: Option<i64>,
    backend_host: Option<String>,
    frontend_host: Option<String>,
    publishable_key: Option<String>,
    mail_from_host: Option<String>,
}

impl StagingDeploymentInsert {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) fn backend_host(mut self, backend_host: impl Into<String>) -> Self {
        self.backend_host = Some(backend_host.into());
        self
    }

    pub(super) fn frontend_host(mut self, frontend_host: impl Into<String>) -> Self {
        self.frontend_host = Some(frontend_host.into());
        self
    }

    pub(super) fn publishable_key(mut self, publishable_key: impl Into<String>) -> Self {
        self.publishable_key = Some(publishable_key.into());
        self
    }

    pub(super) fn mail_from_host(mut self, mail_from_host: impl Into<String>) -> Self {
        self.mail_from_host = Some(mail_from_host.into());
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<StagingDeploymentInsertedRow, AppError> {
        let id = self
            .id
            .ok_or_else(|| AppError::Validation("staging deployment id is required".to_string()))?;
        let project_id = self.project_id.ok_or_else(|| {
            AppError::Validation("staging deployment project_id is required".to_string())
        })?;
        let backend_host = self.backend_host.as_deref().ok_or_else(|| {
            AppError::Validation("staging deployment backend_host is required".to_string())
        })?;
        let frontend_host = self.frontend_host.as_deref().ok_or_else(|| {
            AppError::Validation("staging deployment frontend_host is required".to_string())
        })?;
        let publishable_key = self.publishable_key.as_deref().ok_or_else(|| {
            AppError::Validation("staging deployment publishable_key is required".to_string())
        })?;
        let mail_from_host = self.mail_from_host.as_deref().ok_or_else(|| {
            AppError::Validation("staging deployment mail_from_host is required".to_string())
        })?;

        let now = chrono::Utc::now();

        let row = sqlx::query!(
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
            id,
            project_id,
            "staging",
            backend_host,
            frontend_host,
            publishable_key,
            false,
            mail_from_host,
            now,
            now,
        )
        .fetch_one(conn)
        .await?;

        Ok(StagingDeploymentInsertedRow {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            maintenance_mode: row.maintenance_mode,
            backend_host: row.backend_host,
            frontend_host: row.frontend_host,
            publishable_key: row.publishable_key,
            project_id: row.project_id,
            mode: row.mode,
            mail_from_host: row.mail_from_host,
        })
    }
}

pub(super) struct ProjectWithBillingForStagingRow {
    pub(super) name: String,
    pub(super) status: String,
    pub(super) pulse_usage_disabled: bool,
}

#[derive(Default)]
pub(super) struct ProjectWithBillingForStagingQuery {
    project_id: Option<i64>,
}

impl ProjectWithBillingForStagingQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<Option<ProjectWithBillingForStagingRow>, AppError> {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let row = sqlx::query!(
            r#"
            SELECT p.name, ba.status, COALESCE(ba.pulse_usage_disabled, false) AS "pulse_usage_disabled!"
            FROM projects p
            JOIN billing_accounts ba ON p.billing_account_id = ba.id
            WHERE p.id = $1 AND p.deleted_at IS NULL
            "#,
            project_id
        )
        .fetch_optional(conn)
        .await?;

        Ok(row.map(|r| ProjectWithBillingForStagingRow {
            name: r.name,
            status: r.status,
            pulse_usage_disabled: r.pulse_usage_disabled,
        }))
    }
}

#[derive(Default)]
pub(super) struct StagingDeploymentCountByProjectQuery {
    project_id: Option<i64>,
}

impl StagingDeploymentCountByProjectQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<i64, AppError> {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let row = sqlx::query!(
            "SELECT COUNT(*) as count FROM deployments WHERE project_id = $1 AND mode = 'staging' AND deleted_at IS NULL",
            project_id
        )
        .fetch_one(conn)
        .await?;

        Ok(row.count.unwrap_or(0))
    }
}

pub(super) struct ProjectForProductionRow {
    pub(super) name: String,
    pub(super) status: String,
}

#[derive(Default)]
pub(super) struct ProjectForProductionQuery {
    project_id: Option<i64>,
}

impl ProjectForProductionQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<Option<ProjectForProductionRow>, AppError> {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let row = ProjectWithBillingForStagingQuery::builder()
            .project_id(project_id)
            .execute_with_db(conn)
            .await?;

        Ok(row.map(|r| ProjectForProductionRow {
            name: r.name,
            status: r.status,
        }))
    }
}

#[derive(Default)]
pub(super) struct ExistingProductionDeploymentQuery {
    project_id: Option<i64>,
}

impl ExistingProductionDeploymentQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<Option<i64>, AppError> {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let row = sqlx::query!(
            "SELECT id FROM deployments WHERE project_id = $1 AND mode = 'production' AND deleted_at IS NULL",
            project_id
        )
        .fetch_optional(conn)
        .await?;

        Ok(row.map(|r| r.id))
    }
}

pub(super) struct ExistingDomainDeploymentRow {
    pub(super) id: i64,
}

#[derive(Default)]
pub(super) struct ExistingDomainDeploymentQuery {
    custom_domain: Option<String>,
}

impl ExistingDomainDeploymentQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn custom_domain(mut self, custom_domain: impl Into<String>) -> Self {
        self.custom_domain = Some(custom_domain.into());
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<Option<ExistingDomainDeploymentRow>, AppError> {
        let custom_domain = self
            .custom_domain
            .as_deref()
            .ok_or_else(|| AppError::Validation("custom_domain is required".to_string()))?;

        let row = sqlx::query!(
            "SELECT id FROM deployments WHERE (backend_host = $1 OR frontend_host = $2 OR mail_from_host = $3) AND deleted_at IS NULL",
            format!("frontend.{}", custom_domain),
            format!("accounts.{}", custom_domain),
            custom_domain
        )
        .fetch_optional(conn)
        .await?;

        Ok(row.map(|r| ExistingDomainDeploymentRow { id: r.id }))
    }
}

pub(super) struct ProductionDeploymentInsertedRow {
    pub(super) id: i64,
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
    pub(super) updated_at: chrono::DateTime<chrono::Utc>,
    pub(super) maintenance_mode: bool,
    pub(super) backend_host: String,
    pub(super) frontend_host: String,
    pub(super) publishable_key: String,
    pub(super) project_id: i64,
    pub(super) mode: String,
    pub(super) mail_from_host: String,
    pub(super) email_provider: String,
    pub(super) custom_smtp_config: Option<serde_json::Value>,
}

#[derive(Default)]
pub(super) struct ProductionDeploymentInsert {
    id: Option<i64>,
    project_id: Option<i64>,
    backend_host: Option<String>,
    frontend_host: Option<String>,
    publishable_key: Option<String>,
    mail_from_host: Option<String>,
    domain_verification_records: Option<serde_json::Value>,
    email_verification_records: Option<serde_json::Value>,
}

impl ProductionDeploymentInsert {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) fn backend_host(mut self, backend_host: impl Into<String>) -> Self {
        self.backend_host = Some(backend_host.into());
        self
    }

    pub(super) fn frontend_host(mut self, frontend_host: impl Into<String>) -> Self {
        self.frontend_host = Some(frontend_host.into());
        self
    }

    pub(super) fn publishable_key(mut self, publishable_key: impl Into<String>) -> Self {
        self.publishable_key = Some(publishable_key.into());
        self
    }

    pub(super) fn mail_from_host(mut self, mail_from_host: impl Into<String>) -> Self {
        self.mail_from_host = Some(mail_from_host.into());
        self
    }

    pub(super) fn domain_verification_records(
        mut self,
        domain_verification_records: serde_json::Value,
    ) -> Self {
        self.domain_verification_records = Some(domain_verification_records);
        self
    }

    pub(super) fn email_verification_records(
        mut self,
        email_verification_records: serde_json::Value,
    ) -> Self {
        self.email_verification_records = Some(email_verification_records);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<ProductionDeploymentInsertedRow, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("production deployment id is required".to_string())
        })?;
        let project_id = self.project_id.ok_or_else(|| {
            AppError::Validation("production deployment project_id is required".to_string())
        })?;
        let backend_host = self.backend_host.as_deref().ok_or_else(|| {
            AppError::Validation("production deployment backend_host is required".to_string())
        })?;
        let frontend_host = self.frontend_host.as_deref().ok_or_else(|| {
            AppError::Validation("production deployment frontend_host is required".to_string())
        })?;
        let publishable_key = self.publishable_key.as_deref().ok_or_else(|| {
            AppError::Validation("production deployment publishable_key is required".to_string())
        })?;
        let mail_from_host = self.mail_from_host.as_deref().ok_or_else(|| {
            AppError::Validation("production deployment mail_from_host is required".to_string())
        })?;
        let domain_verification_records =
            self.domain_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation(
                    "production deployment domain_verification_records are required".to_string(),
                )
            })?;
        let email_verification_records =
            self.email_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation(
                    "production deployment email_verification_records are required".to_string(),
                )
            })?;

        let now = chrono::Utc::now();

        let row = sqlx::query!(
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
                     email_provider, custom_smtp_config::jsonb as custom_smtp_config
            "#,
            id,
            project_id,
            "production",
            backend_host,
            frontend_host,
            publishable_key,
            false,
            mail_from_host,
            domain_verification_records,
            email_verification_records,
            now,
            now,
        )
        .fetch_one(conn)
        .await?;

        Ok(ProductionDeploymentInsertedRow {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            maintenance_mode: row.maintenance_mode,
            backend_host: row.backend_host,
            frontend_host: row.frontend_host,
            publishable_key: row.publishable_key,
            project_id: row.project_id,
            mode: row.mode,
            mail_from_host: row.mail_from_host,
            email_provider: row.email_provider,
            custom_smtp_config: row.custom_smtp_config,
        })
    }
}

#[derive(Default)]
pub(super) struct DeploymentEmailVerificationUpdate {
    deployment_id: Option<i64>,
    email_verification_records: Option<serde_json::Value>,
}

impl DeploymentEmailVerificationUpdate {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(super) fn email_verification_records(
        mut self,
        email_verification_records: serde_json::Value,
    ) -> Self {
        self.email_verification_records = Some(email_verification_records);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;
        let email_verification_records =
            self.email_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation("email_verification_records are required".to_string())
            })?;

        sqlx::query!(
            r#"
            UPDATE deployments
            SET email_verification_records = $1, updated_at = $2
            WHERE id = $3
            "#,
            email_verification_records,
            chrono::Utc::now(),
            deployment_id
        )
        .execute(conn)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(super) struct DeploymentDomainVerificationUpdate {
    deployment_id: Option<i64>,
    domain_verification_records: Option<serde_json::Value>,
}

impl DeploymentDomainVerificationUpdate {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(super) fn domain_verification_records(
        mut self,
        domain_verification_records: serde_json::Value,
    ) -> Self {
        self.domain_verification_records = Some(domain_verification_records);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;
        let domain_verification_records =
            self.domain_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation("domain_verification_records are required".to_string())
            })?;

        sqlx::query!(
            r#"
            UPDATE deployments
            SET domain_verification_records = $1, updated_at = $2
            WHERE id = $3
            "#,
            domain_verification_records,
            chrono::Utc::now(),
            deployment_id
        )
        .execute(conn)
        .await?;

        Ok(())
    }
}

pub(super) struct DeploymentByIdRow {
    pub(super) id: i64,
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
    pub(super) updated_at: chrono::DateTime<chrono::Utc>,
    pub(super) maintenance_mode: bool,
    pub(super) backend_host: String,
    pub(super) frontend_host: String,
    pub(super) publishable_key: String,
    pub(super) project_id: i64,
    pub(super) mode: String,
    pub(super) mail_from_host: String,
    pub(super) domain_verification_records: Option<serde_json::Value>,
    pub(super) email_verification_records: Option<serde_json::Value>,
    pub(super) email_provider: String,
    pub(super) custom_smtp_config: Option<serde_json::Value>,
}

#[derive(Default)]
pub(super) struct DeploymentByIdQuery {
    deployment_id: Option<i64>,
}

impl DeploymentByIdQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    #[allow(dead_code)]
    pub(super) async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<DeploymentByIdRow, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        let row = self.execute_with_deps(tx.as_mut()).await?;
        tx.commit().await?;
        Ok(row)
    }

    pub(super) async fn execute_with_deps(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<DeploymentByIdRow, AppError> {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;

        let row = sqlx::query!(
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
            deployment_id
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(DeploymentByIdRow {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            maintenance_mode: row.maintenance_mode,
            backend_host: row.backend_host,
            frontend_host: row.frontend_host,
            publishable_key: row.publishable_key,
            project_id: row.project_id,
            mode: row.mode,
            mail_from_host: row.mail_from_host,
            domain_verification_records: row.domain_verification_records,
            email_verification_records: row.email_verification_records,
            email_provider: row.email_provider,
            custom_smtp_config: row.custom_smtp_config,
        })
    }
}

#[derive(Default)]
pub(super) struct DeploymentDnsRecordsUpdate {
    deployment_id: Option<i64>,
    domain_verification_records: Option<serde_json::Value>,
    email_verification_records: Option<serde_json::Value>,
}

impl DeploymentDnsRecordsUpdate {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(super) fn domain_verification_records(
        mut self,
        domain_verification_records: serde_json::Value,
    ) -> Self {
        self.domain_verification_records = Some(domain_verification_records);
        self
    }

    pub(super) fn email_verification_records(
        mut self,
        email_verification_records: serde_json::Value,
    ) -> Self {
        self.email_verification_records = Some(email_verification_records);
        self
    }

    #[allow(dead_code)]
    pub(super) async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        self.execute_with_deps(tx.as_mut()).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn execute_with_deps(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;
        let domain_verification_records =
            self.domain_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation("domain_verification_records are required".to_string())
            })?;
        let email_verification_records =
            self.email_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation("email_verification_records are required".to_string())
            })?;

        sqlx::query!(
            r#"
            UPDATE deployments
            SET domain_verification_records = $1,
                email_verification_records = $2,
                updated_at = $3
            WHERE id = $4
            "#,
            domain_verification_records,
            email_verification_records,
            chrono::Utc::now(),
            deployment_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(super) struct ActiveDeploymentIdsByProjectQuery {
    project_id: Option<i64>,
}

impl ActiveDeploymentIdsByProjectQuery {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<Vec<i64>, AppError> {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let rows = sqlx::query!(
            r#"
            SELECT id FROM deployments
            WHERE project_id = $1 AND deleted_at IS NULL
            "#,
            project_id
        )
        .fetch_all(conn)
        .await?;

        Ok(rows.into_iter().map(|r| r.id).collect())
    }
}

#[derive(Default)]
pub(super) struct DeleteDeploymentSocialConnectionsByIds {
    deployment_ids: Option<Vec<i64>>,
}

impl DeleteDeploymentSocialConnectionsByIds {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn deployment_ids(mut self, deployment_ids: Vec<i64>) -> Self {
        self.deployment_ids = Some(deployment_ids);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let deployment_ids = self
            .deployment_ids
            .as_ref()
            .ok_or_else(|| AppError::Validation("deployment_ids are required".to_string()))?;

        if deployment_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"
            DELETE FROM deployment_social_connections
            WHERE deployment_id = ANY($1::bigint[])
            "#,
            deployment_ids
        )
        .execute(conn)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(super) struct DeleteDeploymentAuthSettingsByIds {
    deployment_ids: Option<Vec<i64>>,
}

impl DeleteDeploymentAuthSettingsByIds {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn deployment_ids(mut self, deployment_ids: Vec<i64>) -> Self {
        self.deployment_ids = Some(deployment_ids);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let deployment_ids = self
            .deployment_ids
            .as_ref()
            .ok_or_else(|| AppError::Validation("deployment_ids are required".to_string()))?;

        if deployment_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"
            DELETE FROM deployment_auth_settings
            WHERE deployment_id = ANY($1::bigint[])
            "#,
            deployment_ids
        )
        .execute(conn)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(super) struct DeleteDeploymentUiSettingsByIds {
    deployment_ids: Option<Vec<i64>>,
}

impl DeleteDeploymentUiSettingsByIds {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn deployment_ids(mut self, deployment_ids: Vec<i64>) -> Self {
        self.deployment_ids = Some(deployment_ids);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let deployment_ids = self
            .deployment_ids
            .as_ref()
            .ok_or_else(|| AppError::Validation("deployment_ids are required".to_string()))?;

        if deployment_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"
            DELETE FROM deployment_ui_settings
            WHERE deployment_id = ANY($1::bigint[])
            "#,
            deployment_ids
        )
        .execute(conn)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(super) struct DeleteDeploymentB2bSettingsByIds {
    deployment_ids: Option<Vec<i64>>,
}

impl DeleteDeploymentB2bSettingsByIds {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn deployment_ids(mut self, deployment_ids: Vec<i64>) -> Self {
        self.deployment_ids = Some(deployment_ids);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let deployment_ids = self
            .deployment_ids
            .as_ref()
            .ok_or_else(|| AppError::Validation("deployment_ids are required".to_string()))?;

        if deployment_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"
            DELETE FROM deployment_b2b_settings
            WHERE deployment_id = ANY($1::bigint[])
            "#,
            deployment_ids
        )
        .execute(conn)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(super) struct DeleteDeploymentsByProject {
    project_id: Option<i64>,
}

impl DeleteDeploymentsByProject {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        sqlx::query!(
            r#"
            DELETE FROM deployments
            WHERE project_id = $1
            "#,
            project_id
        )
        .execute(conn)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(super) struct DeleteProjectById {
    project_id: Option<i64>,
}

impl DeleteProjectById {
    pub(super) fn builder() -> Self {
        Self::default()
    }

    pub(super) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(super) async fn execute_with_db(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        sqlx::query!(
            r#"
            DELETE FROM projects
            WHERE id = $1
            "#,
            project_id
        )
        .execute(conn)
        .await?;

        Ok(())
    }
}
