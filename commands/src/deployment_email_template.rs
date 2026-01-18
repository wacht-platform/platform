use crate::Command;
use common::error::AppError;
use common::state::AppState;
use dto::params::deployment::DeploymentNameParams;
use models::EmailTemplate;
use scraper::{Html, Selector};

/// Strips HTML wrapper (DOCTYPE, html, body tags) from template content.
/// This ensures we only store the inner content in the database.
fn strip_html_wrapper(html: &str) -> String {
    let content = html.trim();
    
    // If it doesn't look like a full HTML document, return as-is
    if !content.contains("<!DOCTYPE") && !content.contains("<html") && !content.contains("<body") {
        return content.to_string();
    }
    
    // Parse the HTML and extract body content
    let document = Html::parse_document(content);
    
    // Try to get the body element's inner HTML
    if let Ok(body_selector) = Selector::parse("body") {
        if let Some(body) = document.select(&body_selector).next() {
            return body.inner_html().trim().to_string();
        }
    }
    
    // If we can't parse it, return as-is
    content.to_string()
}

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

        if self.template.template_data.contains("<style") {
            return Err(AppError::BadRequest(
                "Email templates cannot contain <style> tags. Please use inline styles.".to_string(),
            ));
        }

        if self.template.template_data.contains("<meta") {
            return Err(AppError::BadRequest(
                "Email templates cannot contain <meta> tags.".to_string(),
            ));
        }

        let query = format!(
            "UPDATE deployment_email_templates SET {} = $1, updated_at = NOW() WHERE deployment_id = $2 AND deleted_at IS NULL",
            column_name
        );

        // Strip HTML wrapper from template_data before saving
        let mut template = self.template;
        template.template_data = strip_html_wrapper(&template.template_data);

        let template_json = serde_json::to_value(&template)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        sqlx::query(&query)
            .bind(template_json)
            .bind(self.deployment_id)
            .execute(&app_state.db_pool)
            .await?;

        // Clear Redis cache for deployment
        use crate::deployment::ClearDeploymentCacheCommand;
        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        Ok(template)
    }
}
