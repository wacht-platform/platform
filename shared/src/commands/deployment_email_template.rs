use crate::{
    error::AppError, state::AppState,
    dto::params::deployment::DeploymentNameParams, models::EmailTemplate,
};

use super::Command;

pub struct UpdateDeploymentEmailTemplateCommand {
    deployment_id: i64,
    template_name: DeploymentNameParams,
    template: EmailTemplate,
}

impl UpdateDeploymentEmailTemplateCommand {
    pub fn new(
        deployment_id: i64,
        template_name: DeploymentNameParams,
        template: EmailTemplate,
    ) -> Self {
        Self {
            deployment_id,
            template_name,
            template,
        }
    }
}

impl Command for UpdateDeploymentEmailTemplateCommand {
    type Output = EmailTemplate;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {

        let column_name = match self.template_name {
            DeploymentNameParams::OrganizationInviteTemplate => "organization_invite_template",
            DeploymentNameParams::VerificationCodeTemplate => "verification_code_template",
            DeploymentNameParams::ResetPasswordCodeTemplate => "reset_password_code_template",
            DeploymentNameParams::PrimaryEmailChangeTemplate => "primary_email_change_template",
            DeploymentNameParams::PasswordChangeTemplate => "password_change_template",
            DeploymentNameParams::PasswordRemoveTemplate => "password_remove_template",
            DeploymentNameParams::SignInFromNewDeviceTemplate => "sign_in_from_new_device_template",
            DeploymentNameParams::MagicLinkTemplate => "magic_link_template",
            DeploymentNameParams::WaitlistSignupTemplate => "waitlist_signup_template",
            DeploymentNameParams::WaitlistInviteTemplate => "waitlist_invite_template",
            DeploymentNameParams::WorkspaceInviteTemplate => "workspace_invite_template",
        };

        let query = format!(
            "UPDATE deployment_email_templates SET {} = $1, updated_at = NOW() WHERE deployment_id = $2 AND deleted_at IS NULL",
            column_name
        );

        let template_json = serde_json::to_value(&self.template)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        sqlx::query(&query)
            .bind(template_json)
            .bind(self.deployment_id)
            .execute(&app_state.db_pool)
            .await?;

        // Clear Redis cache for deployment
        use crate::commands::deployment::ClearDeploymentCacheCommand;
        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        Ok(self.template)
    }
}
