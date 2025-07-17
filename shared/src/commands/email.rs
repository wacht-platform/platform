use std::collections::HashMap;

use crate::{queries::Query, error::AppError, queries::GetEmailTemplateByNameQuery, state::AppState};

use super::Command;

pub struct SendEmailCommand {
    deployment_id: i64,
    template_name: String,
    to_email: String,
    variables: HashMap<String, String>,
}

impl SendEmailCommand {
    pub fn new(
        deployment_id: i64,
        template_name: String,
        to_email: String,
        variables: HashMap<String, String>,
    ) -> Self {
        Self {
            deployment_id,
            template_name,
            to_email,
            variables,
        }
    }
}

impl Command for SendEmailCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let template = GetEmailTemplateByNameQuery::new(self.deployment_id, self.template_name)
            .execute(app_state)
            .await?;

        // Get deployment info to determine mail_from_host
        let deployment = sqlx::query!(
            "SELECT mail_from_host FROM deployments WHERE id = $1",
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let subject = app_state
            .handlebars
            .render_template(&template.template_subject, &self.variables)
            .map_err(|e| AppError::BadRequest(format!("Failed to render subject: {}", e)))?;

        let body_html = app_state
            .handlebars
            .render_template(&template.template_data, &self.variables)
            .map_err(|e| AppError::BadRequest(format!("Failed to render body: {}", e)))?;

        // Create a simple text version by stripping HTML tags (basic implementation)
        let body_text = body_html
            .replace("<br>", "\n")
            .replace("<br/>", "\n")
            .replace("<br />", "\n")
            .replace("</p>", "\n\n")
            .replace("</div>", "\n")
            .replace("</h1>", "\n\n")
            .replace("</h2>", "\n\n")
            .replace("</h3>", "\n\n");

        // Remove remaining HTML tags (simple regex replacement)
        let body_text = regex::Regex::new(r"<[^>]*>")
            .unwrap()
            .replace_all(&body_text, "")
            .to_string();

        let from_email = format!("{}@{}", template.template_from, deployment.mail_from_host);

        // Send email via Postmark
        match app_state.postmark_service.send_email(
            &from_email,
            &self.to_email,
            &subject,
            &body_html,
            Some(&body_text),
        )
        .await
        {
            Ok(response) => {
                tracing::info!(
                    "Email sent successfully via Postmark: {} -> {} (Message ID: {})",
                    from_email,
                    self.to_email,
                    response.message_id
                );
            }
            Err(e) => {
                tracing::error!(
                    "Failed to send email via Postmark: from={}, to={}, error={}",
                    from_email,
                    self.to_email,
                    e
                );
                return Err(e);
            }
        }

        Ok(())
    }
}
