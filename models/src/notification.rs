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
    pub ctas: Option<JsonValue>,

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


