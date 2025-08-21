use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{query, query_as};
use tracing::warn;

use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::notification::{Notification, NotificationSeverity};

// NATS notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub id: i64,
    pub user_id: i64,
    pub deployment_id: i64,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub severity: String,
    pub action_url: Option<String>,
    pub action_label: Option<String>,
    pub created_at: DateTime<Utc>,
}

// =====================================================
// CREATE NOTIFICATION COMMAND
// =====================================================
#[derive(Debug, Clone)]
pub struct CreateNotificationCommand {
    pub deployment_id: i64,
    pub user_id: i64,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub action_url: Option<String>,
    pub action_label: Option<String>,
    pub severity: NotificationSeverity,
    pub metadata: Option<JsonValue>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl CreateNotificationCommand {
    pub fn new(deployment_id: i64, user_id: i64, title: String, body: String) -> Self {
        Self {
            deployment_id,
            user_id,
            organization_id: None,
            workspace_id: None,
            title,
            body,
            action_url: None,
            action_label: None,
            severity: NotificationSeverity::Info,
            metadata: None,
            expires_at: None,
        }
    }

    pub fn with_action(mut self, url: String, label: String) -> Self {
        self.action_url = Some(url);
        self.action_label = Some(label);
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

impl Command for CreateNotificationCommand {
    type Output = Notification;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Create new notification
        let notification = query_as::<_, Notification>(
            r#"
            INSERT INTO notifications (
                deployment_id,
                user_id,
                organization_id,
                workspace_id,
                title,
                body,
                action_url,
                action_label,
                severity,
                metadata,
                expires_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING *
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.user_id)
        .bind(self.organization_id)
        .bind(self.workspace_id)
        .bind(self.title)
        .bind(self.body)
        .bind(self.action_url)
        .bind(self.action_label)
        .bind(&self.severity)
        .bind(self.metadata)
        .bind(self.expires_at)
        .fetch_one(&app_state.db_pool)
        .await?;

        // Publish to NATS for real-time delivery
        let subject = format!(
            "notifications.{}.{}",
            notification.deployment_id, notification.user_id
        );
        let message = NotificationMessage {
            id: notification.id,
            user_id: notification.user_id,
            deployment_id: notification.deployment_id,
            organization_id: notification.organization_id,
            workspace_id: notification.workspace_id,
            title: notification.title.clone(),
            body: notification.body.clone(),
            severity: notification.severity.to_string(),
            action_url: notification.action_url.clone(),
            action_label: notification.action_label.clone(),
            created_at: notification.created_at,
        };

        if let Ok(payload) = serde_json::to_vec(&message) {
            if let Err(e) = app_state.nats_client.publish(subject, payload.into()).await {
                warn!("Failed to publish notification to NATS: {}", e);
                // Don't fail the command, just log the error
            }
        }

        Ok(notification)
    }
}

// =====================================================
// MARK NOTIFICATION AS READ COMMAND
// =====================================================
#[derive(Debug)]
pub struct MarkNotificationReadCommand {
    pub notification_id: i64,
    pub user_id: i64,
}

impl Command for MarkNotificationReadCommand {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .execute(&app_state.db_pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

// =====================================================
// MARK ALL NOTIFICATIONS AS READ COMMAND
// =====================================================
#[derive(Debug)]
pub struct MarkAllNotificationsReadCommand {
    pub user_id: i64,
    pub deployment_id: i64,
}

impl Command for MarkAllNotificationsReadCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .execute(&app_state.db_pool)
        .await?;

        Ok(result.rows_affected() as i64)
    }
}

// =====================================================
// ARCHIVE NOTIFICATION COMMAND
// =====================================================
#[derive(Debug)]
pub struct ArchiveNotificationCommand {
    pub notification_id: i64,
    pub user_id: i64,
}

impl Command for ArchiveNotificationCommand {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .execute(&app_state.db_pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

// =====================================================
// DELETE NOTIFICATION COMMAND (Soft delete via archive)
// =====================================================
#[derive(Debug)]
pub struct DeleteNotificationCommand {
    pub notification_id: i64,
    pub user_id: i64,
}

impl Command for DeleteNotificationCommand {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Use archive as soft delete
        ArchiveNotificationCommand {
            notification_id: self.notification_id,
            user_id: self.user_id,
        }
        .execute(app_state)
        .await
    }
}

// =====================================================
// CLEANUP EXPIRED NOTIFICATIONS COMMAND
// =====================================================
#[derive(Debug)]
pub struct CleanupExpiredNotificationsCommand;

impl Command for CleanupExpiredNotificationsCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = query!(
            r#"
            DELETE FROM notifications 
            WHERE expires_at < NOW()
            "#
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(result.rows_affected() as i64)
    }
}
