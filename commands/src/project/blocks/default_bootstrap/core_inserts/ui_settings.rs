use super::*;
pub(in crate::project) struct DeploymentUiSettingsInsert {
    id: i64,
    ui_settings: DeploymentUISettings,
    waitlist_page_url: String,
    support_page_url: String,
}

#[derive(Default)]
pub(in crate::project) struct DeploymentUiSettingsInsertBuilder {
    id: Option<i64>,
    ui_settings: Option<DeploymentUISettings>,
    waitlist_page_url: Option<String>,
    support_page_url: Option<String>,
}

impl DeploymentUiSettingsInsert {
    pub(in crate::project) fn builder() -> DeploymentUiSettingsInsertBuilder {
        DeploymentUiSettingsInsertBuilder::default()
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
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
    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn ui_settings(mut self, ui_settings: DeploymentUISettings) -> Self {
        self.ui_settings = Some(ui_settings);
        self
    }

    pub(in crate::project) fn waitlist_page_url(mut self, waitlist_page_url: impl Into<String>) -> Self {
        self.waitlist_page_url = Some(waitlist_page_url.into());
        self
    }

    pub(in crate::project) fn support_page_url(mut self, support_page_url: impl Into<String>) -> Self {
        self.support_page_url = Some(support_page_url.into());
        self
    }

    pub(in crate::project) fn build(self) -> Result<DeploymentUiSettingsInsert, AppError> {
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

