use crate::notification::{Notification, NotificationSeverity};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// Request DTOs
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateNotificationRequest {
    pub user_id: Option<i64>,       // Single user
    pub user_ids: Option<Vec<i64>>, // Multiple users
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,

    pub title: String,
    pub body: String,

    pub ctas: Option<JsonValue>,

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
