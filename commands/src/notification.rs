use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{query, query_as};
use tracing::warn;

use common::error::AppError;
use models::notification::{Notification, NotificationRow, NotificationSeverity};

// NATS notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub id: i64,
    pub user_id: Option<i64>,
    pub deployment_id: i64,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub severity: String,
    pub ctas: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateNotificationCommand {
    deployment_id: i64,
    user_id: Option<i64>,
    organization_id: Option<i64>,
    workspace_id: Option<i64>,
    title: String,
    body: String,
    ctas: Option<JsonValue>,
    severity: NotificationSeverity,
    metadata: Option<JsonValue>,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Default)]
pub struct CreateNotificationCommandBuilder {
    deployment_id: Option<i64>,
    user_id: Option<i64>,
    organization_id: Option<i64>,
    workspace_id: Option<i64>,
    title: Option<String>,
    body: Option<String>,
    ctas: Option<JsonValue>,
    severity: Option<NotificationSeverity>,
    metadata: Option<JsonValue>,
    expires_at: Option<DateTime<Utc>>,
}

impl CreateNotificationCommand {
    pub fn builder() -> CreateNotificationCommandBuilder {
        CreateNotificationCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, title: String, body: String) -> Self {
        Self {
            deployment_id,
            user_id: None,
            organization_id: None,
            workspace_id: None,
            title,
            body,
            ctas: None,
            severity: NotificationSeverity::Info,
            metadata: None,
            expires_at: None,
        }
    }

    pub fn with_user(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_ctas(mut self, ctas: JsonValue) -> Self {
        self.ctas = Some(ctas);
        self
    }

    pub fn with_severity(mut self, severity: NotificationSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_expiry_hours(mut self, hours: i64) -> Self {
        self.expires_at = Some(Utc::now() + chrono::Duration::hours(hours));
        self
    }

    pub fn with_metadata(mut self, metadata: JsonValue) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_organization(mut self, org_id: i64) -> Self {
        self.organization_id = Some(org_id);
        self
    }

    pub fn with_workspace(mut self, workspace_id: i64) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }
}

impl CreateNotificationCommand {
    pub async fn execute_with(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
        nats_client: &async_nats::Client,
    ) -> Result<Notification, AppError> {
        let mut conn = acquirer.acquire().await?;
        // Create new notification
        let row: NotificationRow = query_as(
            r#"
            INSERT INTO notifications (
                deployment_id,
                user_id,
                organization_id,
                workspace_id,
                title,
                body,
                ctas,
                severity,
                metadata,
                expires_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, COALESCE($10, NOW() + INTERVAL '90 days'))
            RETURNING *
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.user_id)
        .bind(self.organization_id)
        .bind(self.workspace_id)
        .bind(self.title)
        .bind(self.body)
        .bind(self.ctas)
        .bind(&self.severity)
        .bind(self.metadata)
        .bind(self.expires_at)
        .fetch_one(&mut *conn)
        .await?;

        // Convert row to strongly typed Notification
        let notification = Notification::try_from(row)
            .map_err(|e| AppError::Internal(format!("Failed to convert notification: {}", e)))?;

        // Publish to NATS for real-time delivery
        if let Some(user_id) = notification.user_id {
            let subject = format!("notifications.{}.{}", notification.deployment_id, user_id);

            // Serialize ctas back to JSON for NATS message
            let ctas_json = notification
                .ctas
                .as_ref()
                .and_then(|ctas| serde_json::to_value(ctas).ok());

            let message = NotificationMessage {
                id: notification.id,
                user_id: notification.user_id,
                deployment_id: notification.deployment_id,
                organization_id: notification.organization_id,
                workspace_id: notification.workspace_id,
                title: notification.title.clone(),
                body: notification.body.clone(),
                severity: notification.severity.to_string(),
                ctas: ctas_json,
                created_at: notification.created_at,
            };

            if let Ok(payload) = serde_json::to_vec(&message) {
                if let Err(e) = nats_client.publish(subject, payload.into()).await {
                    warn!("Failed to publish notification to NATS: {}", e);
                    // Don't fail the command, just log the error
                }
            }
        }

        Ok(notification)
    }
}

impl CreateNotificationCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn user_id(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn organization_id(mut self, organization_id: i64) -> Self {
        self.organization_id = Some(organization_id);
        self
    }

    pub fn workspace_id(mut self, workspace_id: i64) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    pub fn title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    pub fn body(mut self, body: String) -> Self {
        self.body = Some(body);
        self
    }

    pub fn ctas(mut self, ctas: JsonValue) -> Self {
        self.ctas = Some(ctas);
        self
    }

    pub fn severity(mut self, severity: NotificationSeverity) -> Self {
        self.severity = Some(severity);
        self
    }

    pub fn metadata(mut self, metadata: JsonValue) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn expiry_hours(mut self, hours: i64) -> Self {
        self.expires_at = Some(Utc::now() + chrono::Duration::hours(hours));
        self
    }

    pub fn build(self) -> Result<CreateNotificationCommand, AppError> {
        Ok(CreateNotificationCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            user_id: self.user_id,
            organization_id: self.organization_id,
            workspace_id: self.workspace_id,
            title: self
                .title
                .ok_or_else(|| AppError::Validation("title is required".into()))?,
            body: self
                .body
                .ok_or_else(|| AppError::Validation("body is required".into()))?,
            ctas: self.ctas,
            severity: self.severity.unwrap_or(NotificationSeverity::Info),
            metadata: self.metadata,
            expires_at: self.expires_at,
        })
    }
}

#[derive(Debug)]
pub struct MarkNotificationReadCommand {
    notification_id: i64,
    user_id: i64,
}

#[derive(Default)]
pub struct MarkNotificationReadCommandBuilder {
    notification_id: Option<i64>,
    user_id: Option<i64>,
}

impl MarkNotificationReadCommand {
    pub fn builder() -> MarkNotificationReadCommandBuilder {
        MarkNotificationReadCommandBuilder::default()
    }

    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let result = query!(
            r#"
            UPDATE notifications 
            SET 
                is_read = true,
                read_at = NOW(),
                updated_at = NOW()
            WHERE id = $1 AND user_id = $2 AND is_read = false
            "#,
            self.notification_id,
            self.user_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

impl MarkNotificationReadCommandBuilder {
    pub fn notification_id(mut self, notification_id: i64) -> Self {
        self.notification_id = Some(notification_id);
        self
    }

    pub fn user_id(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn build(self) -> Result<MarkNotificationReadCommand, AppError> {
        Ok(MarkNotificationReadCommand {
            notification_id: self
                .notification_id
                .ok_or_else(|| AppError::Validation("notification_id is required".into()))?,
            user_id: self
                .user_id
                .ok_or_else(|| AppError::Validation("user_id is required".into()))?,
        })
    }
}

#[derive(Debug)]
pub struct MarkAllNotificationsReadCommand {
    user_id: i64,
    deployment_id: i64,
}

#[derive(Default)]
pub struct MarkAllNotificationsReadCommandBuilder {
    user_id: Option<i64>,
    deployment_id: Option<i64>,
}

impl MarkAllNotificationsReadCommand {
    pub fn builder() -> MarkAllNotificationsReadCommandBuilder {
        MarkAllNotificationsReadCommandBuilder::default()
    }

    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<i64, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let result = query!(
            r#"
            UPDATE notifications 
            SET 
                is_read = true,
                read_at = NOW(),
                updated_at = NOW()
            WHERE user_id = $1 
            AND deployment_id = $2
            AND is_read = false
            AND is_archived = false
            "#,
            self.user_id,
            self.deployment_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(result.rows_affected() as i64)
    }
}

impl MarkAllNotificationsReadCommandBuilder {
    pub fn user_id(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn build(self) -> Result<MarkAllNotificationsReadCommand, AppError> {
        Ok(MarkAllNotificationsReadCommand {
            user_id: self
                .user_id
                .ok_or_else(|| AppError::Validation("user_id is required".into()))?,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
        })
    }
}

#[derive(Debug)]
pub struct ArchiveNotificationCommand {
    notification_id: i64,
    user_id: i64,
}

#[derive(Default)]
pub struct ArchiveNotificationCommandBuilder {
    notification_id: Option<i64>,
    user_id: Option<i64>,
}

impl ArchiveNotificationCommand {
    pub fn builder() -> ArchiveNotificationCommandBuilder {
        ArchiveNotificationCommandBuilder::default()
    }

    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<bool, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let result = query!(
            r#"
            UPDATE notifications 
            SET 
                is_archived = true,
                archived_at = NOW(),
                updated_at = NOW()
            WHERE id = $1 AND user_id = $2 AND is_archived = false
            "#,
            self.notification_id,
            self.user_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

impl ArchiveNotificationCommandBuilder {
    pub fn notification_id(mut self, notification_id: i64) -> Self {
        self.notification_id = Some(notification_id);
        self
    }

    pub fn user_id(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn build(self) -> Result<ArchiveNotificationCommand, AppError> {
        Ok(ArchiveNotificationCommand {
            notification_id: self
                .notification_id
                .ok_or_else(|| AppError::Validation("notification_id is required".into()))?,
            user_id: self
                .user_id
                .ok_or_else(|| AppError::Validation("user_id is required".into()))?,
        })
    }
}

#[derive(Debug)]
pub struct DeleteNotificationCommand {
    notification_id: i64,
    user_id: i64,
}

#[derive(Default)]
pub struct DeleteNotificationCommandBuilder {
    notification_id: Option<i64>,
    user_id: Option<i64>,
}

impl DeleteNotificationCommand {
    pub fn builder() -> DeleteNotificationCommandBuilder {
        DeleteNotificationCommandBuilder::default()
    }

    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        ArchiveNotificationCommand::builder()
            .notification_id(self.notification_id)
            .user_id(self.user_id)
            .build()?
            .execute_with_deps(&mut *conn)
            .await
    }
}

impl DeleteNotificationCommandBuilder {
    pub fn notification_id(mut self, notification_id: i64) -> Self {
        self.notification_id = Some(notification_id);
        self
    }

    pub fn user_id(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn build(self) -> Result<DeleteNotificationCommand, AppError> {
        Ok(DeleteNotificationCommand {
            notification_id: self
                .notification_id
                .ok_or_else(|| AppError::Validation("notification_id is required".into()))?,
            user_id: self
                .user_id
                .ok_or_else(|| AppError::Validation("user_id is required".into()))?,
        })
    }
}

#[derive(Debug)]
pub struct CleanupExpiredNotificationsCommand;

impl CleanupExpiredNotificationsCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<i64, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let result = query!(
            r#"
            DELETE FROM notifications 
            WHERE expires_at < NOW()
            "#
        )
        .execute(&mut *conn)
        .await?;

        Ok(result.rows_affected() as i64)
    }
}
