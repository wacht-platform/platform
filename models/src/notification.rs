use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::FromRow;
use std::collections::HashMap;

// Call to action for notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToAction {
    pub label: String,
    pub payload: String,
}

// Main notification model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,

    // Recipients
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub user_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub organization_id: Option<i64>,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub workspace_id: Option<i64>,

    // Content
    pub title: String,
    pub body: String,

    // Action (strongly typed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctas: Option<Vec<CallToAction>>,

    // Severity
    pub severity: NotificationSeverity,

    // Status
    #[serde(rename = "is_read")]
    pub is_read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "read_at")]
    pub read_at: Option<DateTime<Utc>>,
    #[serde(rename = "is_archived")]
    pub is_archived: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "archived_at")]
    pub archived_at: Option<DateTime<Utc>>,

    // Metadata (flexible key-value storage)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, JsonValue>>,

    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

// Database row representation (with JSONB fields)
#[derive(Debug, Clone, FromRow)]
pub struct NotificationRow {
    pub id: i64,
    pub deployment_id: i64,
    pub user_id: i64,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub ctas: Option<JsonValue>,
    pub severity: String,
    pub is_read: bool,
    pub read_at: Option<DateTime<Utc>>,
    pub is_archived: bool,
    pub archived_at: Option<DateTime<Utc>>,
    pub metadata: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl TryFrom<NotificationRow> for Notification {
    type Error = String;

    fn try_from(row: NotificationRow) -> Result<Self, Self::Error> {
        let ctas = if let Some(json_val) = row.ctas {
            let ctas: Vec<CallToAction> = serde_json::from_value(json_val)
                .map_err(|e| format!("Failed to deserialize ctas: {}", e))?;
            Some(ctas)
        } else {
            None
        };

        let metadata = if let Some(json_val) = row.metadata {
            let metadata: HashMap<String, JsonValue> = serde_json::from_value(json_val)
                .map_err(|e| format!("Failed to deserialize metadata: {}", e))?;
            Some(metadata)
        } else {
            None
        };

        Ok(Notification {
            id: row.id,
            deployment_id: row.deployment_id,
            user_id: row.user_id,
            organization_id: row.organization_id,
            workspace_id: row.workspace_id,
            title: row.title,
            body: row.body,
            ctas,
            severity: NotificationSeverity::from(&row.severity),
            is_read: row.is_read,
            read_at: row.read_at,
            is_archived: row.is_archived,
            archived_at: row.archived_at,
            metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
            expires_at: row.expires_at,
        })
    }
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
