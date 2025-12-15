use crate::Command;
use common::error::AppError;
use common::smtp::{SmtpConfig, SmtpService};
use common::state::AppState;
use models::{CustomSmtpConfig, EmailProvider};
use queries::{GetEmailTemplateByNameQuery, Query};

pub struct SendEmailCommand {
    deployment_id: i64,
    template_name: String,
    to_email: String,
    variables: serde_json::Value,
}

impl SendEmailCommand {
    pub fn new(
        deployment_id: i64,
        template_name: String,
        to_email: String,
        variables: serde_json::Value,
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

        let deployment = sqlx::query!(
            r#"
            SELECT mail_from_host, email_provider, custom_smtp_config
            FROM deployments WHERE id = $1
            "#,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let display_settings = sqlx::query!(
            "SELECT app_name from deployment_ui_settings where deployment_id = $1",
            self.deployment_id,
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

        let body_text = html2text::from_read(body_html.as_bytes(), 80)
            .unwrap_or_else(|_| body_html.clone());

        let from_email = format!(
            "{} <notification@{}>",
            display_settings.app_name, deployment.mail_from_host
        );

        let email_provider = EmailProvider::from(deployment.email_provider);

        let smtp_config: Option<CustomSmtpConfig> = deployment
            .custom_smtp_config
            .and_then(|v| serde_json::from_value(v).ok());

        if email_provider == EmailProvider::CustomSmtp {
            if let Some(config) = &smtp_config {
                if config.verified {
                    let decrypted_password = app_state
                        .encryption_service
                        .decrypt(&config.password)
                        .map_err(|e| {
                            tracing::error!("Failed to decrypt SMTP password: {}", e);
                            e
                        })?;

                    let smtp_service = SmtpService::new(SmtpConfig {
                        host: config.host.clone(),
                        port: config.port,
                        username: config.username.clone(),
                        password: decrypted_password,
                        from_email: config.from_email.clone(),
                        use_tls: config.use_tls,
                    });

                    let smtp_from_email = format!(
                        "{} <{}>",
                        display_settings.app_name, config.from_email
                    );

                    match smtp_service
                        .send_email(&smtp_from_email, &self.to_email, &subject, &body_html, Some(&body_text))
                        .await
                    {
                        Ok(_) => {
                            tracing::info!(
                                "Email sent successfully via custom SMTP: {} -> {}",
                                smtp_from_email,
                                self.to_email
                            );
                            return Ok(());
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to send via custom SMTP, falling back to Postmark: {}",
                                e
                            );
                        }
                    }
                }
            }
        }

        match app_state
            .postmark_service
            .send_email(
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
