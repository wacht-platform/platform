use crate::notification::{Notification, NotificationSeverity};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateNotificationRequest {
    pub user_id: Option<i64>,
    pub user_ids: Option<Vec<i64>>,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub ctas: Option<JsonValue>,
    pub severity: Option<NotificationSeverity>,
    pub metadata: Option<JsonValue>,
    pub expires_in_hours: Option<i32>,
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
