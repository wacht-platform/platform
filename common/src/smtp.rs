use crate::error::AppError;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    message::{Mailbox, Message, MultiPart, SinglePart, header::ContentType},
    transport::smtp::authentication::Credentials,
};

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_email: String,
    pub use_tls: bool,
}

#[derive(Clone)]
pub struct SmtpService {
    config: SmtpConfig,
}

impl SmtpService {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }

    async fn create_transport(&self) -> Result<AsyncSmtpTransport<Tokio1Executor>, AppError> {
        let creds = Credentials::new(self.config.username.clone(), self.config.password.clone());

        let transport = if self.config.use_tls {
            // STARTTLS: starts unencrypted, upgrades to TLS (typically port 587)
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.config.host)
                .map_err(|e| AppError::External(format!("Failed to create SMTP transport: {}", e)))?
                .port(self.config.port)
                .credentials(creds)
                .build()
        } else {
            // Implicit TLS/SSL: encrypted from the start (typically port 465)
            AsyncSmtpTransport::<Tokio1Executor>::relay(&self.config.host)
                .map_err(|e| AppError::External(format!("Failed to create SMTP transport: {}", e)))?
                .port(self.config.port)
                .credentials(creds)
                .build()
        };

        Ok(transport)
    }

    pub async fn test_connection(&self) -> Result<(), AppError> {
        let transport = self.create_transport().await?;

        transport
            .test_connection()
            .await
            .map_err(|e| AppError::External(format!("SMTP connection test failed: {}", e)))?;

        tracing::info!(
            "SMTP connection test successful for {}:{}",
            self.config.host,
            self.config.port
        );

        Ok(())
    }

    pub async fn send_email(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        html_body: &str,
        text_body: Option<&str>,
    ) -> Result<SmtpSendResponse, AppError> {
        let from_mailbox: Mailbox = from
            .parse()
            .map_err(|e| AppError::BadRequest(format!("Invalid from address: {}", e)))?;

        let to_mailbox: Mailbox = to
            .parse()
            .map_err(|e| AppError::BadRequest(format!("Invalid to address: {}", e)))?;

        let message_builder = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(subject);

        let message = if let Some(text) = text_body {
            message_builder
                .multipart(
                    MultiPart::alternative()
                        .singlepart(
                            SinglePart::builder()
                                .header(ContentType::TEXT_PLAIN)
                                .body(text.to_string()),
                        )
                        .singlepart(
                            SinglePart::builder()
                                .header(ContentType::TEXT_HTML)
                                .body(html_body.to_string()),
                        ),
                )
                .map_err(|e| AppError::Internal(format!("Failed to build email: {}", e)))?
        } else {
            message_builder
                .header(ContentType::TEXT_HTML)
                .body(html_body.to_string())
                .map_err(|e| AppError::Internal(format!("Failed to build email: {}", e)))?
        };

        let transport = self.create_transport().await?;

        let response = transport
            .send(message)
            .await
            .map_err(|e| AppError::External(format!("Failed to send email via SMTP: {}", e)))?;

        tracing::info!(
            "Email sent successfully via SMTP: {} -> {} (code: {:?})",
            from,
            to,
            response.code()
        );

        Ok(SmtpSendResponse {
            message: response.message().collect::<Vec<_>>().join(" "),
            code: response.code().to_string(),
        })
    }

    pub fn from_email(&self) -> &str {
        &self.config.from_email
    }
}

pub struct SmtpSendResponse {
    pub message: String,
    pub code: String,
}
