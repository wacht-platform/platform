use crate::Command;
use common::error::AppError;
use common::state::AppState;
use dto::json::DeploymentDisplaySettingsUpdates;

use super::ClearDeploymentCacheCommand;

pub struct UpdateDeploymentDisplaySettingsCommand {
    deployment_id: i64,
    settings: DeploymentDisplaySettingsUpdates,
}

impl UpdateDeploymentDisplaySettingsCommand {
    pub fn new(deployment_id: i64, settings: DeploymentDisplaySettingsUpdates) -> Self {
        Self {
            deployment_id,
            settings,
        }
    }
}

impl UpdateDeploymentDisplaySettingsCommand {
    pub async fn execute_with(self, app_state: &AppState) -> Result<(), AppError> {
        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_ui_settings SET updated_at = NOW() ");

        if let Some(app_name) = &self.settings.app_name {
            query_builder.push(", app_name = ");
            query_builder.push_bind(app_name);
        }

        if let Some(tos_page_url) = &self.settings.tos_page_url {
            query_builder.push(", tos_page_url = ");
            query_builder.push_bind(tos_page_url);
        }

        if let Some(sign_in_page_url) = &self.settings.sign_in_page_url {
            query_builder.push(", sign_in_page_url = ");
            query_builder.push_bind(sign_in_page_url);
        }

        if let Some(sign_up_page_url) = &self.settings.sign_up_page_url {
            query_builder.push(", sign_up_page_url = ");
            query_builder.push_bind(sign_up_page_url);
        }

        if let Some(after_sign_out_one_page_url) = &self.settings.after_sign_out_one_page_url {
            query_builder.push(", after_sign_out_one_page_url = ");
            query_builder.push_bind(after_sign_out_one_page_url);
        }

        if let Some(after_sign_out_all_page_url) = &self.settings.after_sign_out_all_page_url {
            query_builder.push(", after_sign_out_all_page_url = ");
            query_builder.push_bind(after_sign_out_all_page_url);
        }

        if let Some(favicon_image_url) = &self.settings.favicon_image_url {
            query_builder.push(", favicon_image_url = ");
            query_builder.push_bind(favicon_image_url);
        }

        if let Some(logo_image_url) = &self.settings.logo_image_url {
            query_builder.push(", logo_image_url = ");
            query_builder.push_bind(logo_image_url);
        }

        if let Some(privacy_policy_url) = &self.settings.privacy_policy_url {
            query_builder.push(", privacy_policy_url = ");
            query_builder.push_bind(privacy_policy_url);
        }

        if let Some(signup_terms_statement) = &self.settings.signup_terms_statement {
            query_builder.push(", signup_terms_statement = ");
            query_builder.push_bind(signup_terms_statement);
        }

        if let Some(signup_terms_statement_shown) = &self.settings.signup_terms_statement_shown {
            query_builder.push(", signup_terms_statement_shown = ");
            query_builder.push_bind(signup_terms_statement_shown);
        }

        if let Some(light_mode_settings) = &self.settings.light_mode_settings {
            query_builder.push(", light_mode_settings = ");
            query_builder.push_bind(serde_json::to_value(light_mode_settings)?);
        }

        if let Some(dark_mode_settings) = &self.settings.dark_mode_settings {
            query_builder.push(", dark_mode_settings = ");
            query_builder.push_bind(serde_json::to_value(dark_mode_settings)?);
        }

        if let Some(after_logo_click_url) = &self.settings.after_logo_click_url {
            query_builder.push(", after_logo_click_url = ");
            query_builder.push_bind(after_logo_click_url);
        }

        if let Some(organization_profile_url) = &self.settings.organization_profile_url {
            query_builder.push(", organization_profile_url = ");
            query_builder.push_bind(organization_profile_url);
        }

        if let Some(create_organization_url) = &self.settings.create_organization_url {
            query_builder.push(", create_organization_url = ");
            query_builder.push_bind(create_organization_url);
        }

        if let Some(default_user_profile_image_url) = &self.settings.default_user_profile_image_url {
            query_builder.push(", default_user_profile_image_url = ");
            query_builder.push_bind(default_user_profile_image_url);
        }

        if let Some(default_organization_profile_image_url) =
            &self.settings.default_organization_profile_image_url
        {
            query_builder.push(", default_organization_profile_image_url = ");
            query_builder.push_bind(default_organization_profile_image_url);
        }

        if let Some(use_initials_for_user_profile_image) =
            &self.settings.use_initials_for_user_profile_image
        {
            query_builder.push(", use_initials_for_user_profile_image = ");
            query_builder.push_bind(use_initials_for_user_profile_image);
        }

        if let Some(use_initials_for_organization_profile_image) =
            &self.settings.use_initials_for_organization_profile_image
        {
            query_builder.push(", use_initials_for_organization_profile_image = ");
            query_builder.push_bind(use_initials_for_organization_profile_image);
        }

        if let Some(after_signup_redirect_url) = &self.settings.after_signup_redirect_url {
            query_builder.push(", after_signup_redirect_url = ");
            query_builder.push_bind(after_signup_redirect_url);
        }

        if let Some(after_signin_redirect_url) = &self.settings.after_signin_redirect_url {
            query_builder.push(", after_signin_redirect_url = ");
            query_builder.push_bind(after_signin_redirect_url);
        }

        if let Some(user_profile_url) = &self.settings.user_profile_url {
            query_builder.push(", user_profile_url = ");
            query_builder.push_bind(user_profile_url);
        }

        if let Some(after_create_organization_redirect_url) =
            &self.settings.after_create_organization_redirect_url
        {
            query_builder.push(", after_create_organization_redirect_url = ");
            query_builder.push_bind(after_create_organization_redirect_url);
        }

        if let Some(default_workspace_profile_image_url) =
            &self.settings.default_workspace_profile_image_url
        {
            query_builder.push(", default_workspace_profile_image_url = ");
            query_builder.push_bind(default_workspace_profile_image_url);
        }

        if let Some(waitlist_page_url) = &self.settings.waitlist_page_url {
            query_builder.push(", waitlist_page_url = ");
            query_builder.push_bind(waitlist_page_url);
        }

        if let Some(support_page_url) = &self.settings.support_page_url {
            query_builder.push(", support_page_url = ");
            query_builder.push_bind(support_page_url);
        }

        query_builder.push(" WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);

        let result = query_builder.build().execute(&app_state.db_pool).await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "Display settings for deployment {} not found",
                self.deployment_id
            )));
        }

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute_with(app_state)
            .await?;

        Ok(())
    }
}

impl Command for UpdateDeploymentDisplaySettingsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}
