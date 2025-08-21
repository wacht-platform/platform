use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::FromRow;

// Main notification model matching the database schema
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Notification {
    pub id: i64,
    pub deployment_id: i64,

    // Recipients
    pub user_id: i64,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,

    // Content
    pub title: String,
    pub body: String,

    // Action
    pub action_url: Option<String>,
    pub action_label: Option<String>,

    // Severity
    pub severity: NotificationSeverity,

    // Status
    pub is_read: bool,
    pub read_at: Option<DateTime<Utc>>,
    pub is_archived: bool,
    pub archived_at: Option<DateTime<Utc>>,

    // Metadata
    pub metadata: Option<JsonValue>,

    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

// Severity enum for notification types
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "varchar")]
#[sqlx(rename_all = "lowercase")]
pub enum NotificationSeverity {
    #[serde(rename = "info")]
    #[sqlx(rename = "info")]
    Info,

    #[serde(rename = "success")]
    #[sqlx(rename = "success")]
    Success,

    #[serde(rename = "warning")]
    #[sqlx(rename = "warning")]
    Warning,

    #[serde(rename = "error")]
    #[sqlx(rename = "error")]
    Error,
}

impl Default for NotificationSeverity {
    fn default() -> Self {
        NotificationSeverity::Info
    }
}

impl NotificationSeverity {
    pub fn from(s: &str) -> Self {
        match s {
            "success" => NotificationSeverity::Success,
            "warning" => NotificationSeverity::Warning,
            "error" => NotificationSeverity::Error,
            _ => NotificationSeverity::Info,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            NotificationSeverity::Info => "info".to_string(),
            NotificationSeverity::Success => "success".to_string(),
            NotificationSeverity::Warning => "warning".to_string(),
            NotificationSeverity::Error => "error".to_string(),
        }
    }
}

// Request DTOs
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateNotificationRequest {
    pub user_id: Option<i64>,       // Single user
    pub user_ids: Option<Vec<i64>>, // Multiple users
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,

    pub title: String,
    pub body: String,

    pub action_url: Option<String>,
    pub action_label: Option<String>,

    pub severity: Option<NotificationSeverity>,

    pub metadata: Option<JsonValue>,
    pub expires_in_hours: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateNotificationRequest {
    pub is_read: Option<bool>,
    pub is_archived: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkUpdateRequest {
    pub notification_ids: Option<Vec<i64>>, // Specific IDs
    pub mark_all: Option<bool>,             // Or mark all
    pub is_read: Option<bool>,
    pub is_archived: Option<bool>,
}

// Response DTOs
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationListResponse {
    pub notifications: Vec<Notification>,
    pub total: i64,
    pub unread_count: i64,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UnreadCountResponse {
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkUpdateResponse {
    pub affected: i64,
}

// Query parameters for listing notifications
#[derive(Debug, Deserialize)]
pub struct NotificationListParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub is_read: Option<bool>,
    pub is_archived: Option<bool>,
    pub severity: Option<NotificationSeverity>,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
}

impl Default for NotificationListParams {
    fn default() -> Self {
        Self {
            limit: Some(20),
            offset: Some(0),
            is_read: None,
            is_archived: None,
            severity: None,
            organization_id: None,
            workspace_id: None,
        }
    }
}

// WebSocket event types for real-time updates
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum NotificationEvent {
    New { notification: Notification },
    Updated { notification: Notification },
    Read { notification_id: i64 },
    Archived { notification_id: i64 },
    Deleted { notification_id: i64 },
    BulkRead { notification_ids: Vec<i64> },
    UnreadCountChanged { count: i64 },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationWebSocketMessage {
    pub event: NotificationEvent,
    pub user_id: i64,
    pub timestamp: DateTime<Utc>,
}
