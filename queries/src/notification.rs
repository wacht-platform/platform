use common::error::AppError;
use models::notification::{
    Notification, NotificationListParams, NotificationListResponse, NotificationSeverity,
};
use common::state::AppState;
use sqlx::{Row, query, query_as};

use super::Query;

// =====================================================
// GET USER NOTIFICATIONS QUERY
// =====================================================
#[derive(Debug)]
pub struct GetUserNotificationsQuery {
    pub user_id: i64,
    pub deployment_id: i64,
    pub params: NotificationListParams,
}

impl Query for GetUserNotificationsQuery {
    type Output = NotificationListResponse;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.params.limit.unwrap_or(20);
        let offset = self.params.offset.unwrap_or(0);

        // Build base query
        let mut conditions = vec![
            "n.user_id = $1".to_string(),
            "n.deployment_id = $2".to_string(),
        ];

        // Add filters
        if let Some(is_read) = self.params.is_read {
            conditions.push(format!("n.is_read = {}", is_read));
        }

        if let Some(is_archived) = self.params.is_archived {
            conditions.push(format!("n.is_archived = {}", is_archived));
        } else {
            conditions.push("n.is_archived = false".to_string());
        }

        // Filter out expired
        conditions.push("(n.expires_at IS NULL OR n.expires_at > NOW())".to_string());

        let where_clause = conditions.join(" AND ");

        // Get notifications
        let query_str = format!(
            r#"
            SELECT * FROM notifications n
            WHERE {}
            ORDER BY n.created_at DESC
            LIMIT $3 OFFSET $4
            "#,
            where_clause
        );

        let mut notifications = Vec::new();
        let rows = sqlx::query(&query_str)
            .bind(self.user_id)
            .bind(self.deployment_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&app_state.db_pool)
            .await?;

        for row in rows {
            notifications.push(Notification {
                id: row.get("id"),
                deployment_id: row.get("deployment_id"),
                user_id: row.get("user_id"),
                organization_id: row.get("organization_id"),
                workspace_id: row.get("workspace_id"),
                title: row.get("title"),
                body: row.get("body"),
                action_url: row.get("action_url"),
                action_label: row.get("action_label"),
                severity: {
                    let s: String = row.get("severity");
                    match s.as_str() {
                        "success" => NotificationSeverity::Success,
                        "warning" => NotificationSeverity::Warning,
                        "error" => NotificationSeverity::Error,
                        _ => NotificationSeverity::Info,
                    }
                },
                is_read: row.get("is_read"),
                read_at: row.get("read_at"),
                is_archived: row.get("is_archived"),
                archived_at: row.get("archived_at"),
                group_id: row.get("group_id"),
                group_count: row.get("group_count"),
                dedupe_key: row.get("dedupe_key"),
                source: row.get("source"),
                source_id: row.get("source_id"),
                metadata: row.get("metadata"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                expires_at: row.get("expires_at"),
            });
        }

        // Get counts
        let count_result: (i64, i64) = query_as(
            r#"
            SELECT
                COUNT(*) as total,
                COUNT(*) FILTER (WHERE is_read = false AND is_archived = false) as unread
            FROM notifications
            WHERE user_id = $1 AND deployment_id = $2
            AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .bind(self.user_id)
        .bind(self.deployment_id)
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(NotificationListResponse {
            notifications,
            total: count_result.0,
            unread_count: count_result.1,
            has_more: (offset + limit) < count_result.0,
        })
    }
}

// =====================================================
// GET UNREAD COUNT QUERY
// =====================================================
#[derive(Debug)]
pub struct GetUnreadCountQuery {
    pub user_id: i64,
    pub deployment_id: i64,
}

impl Query for GetUnreadCountQuery {
    type Output = i64;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let count: (i64,) = query_as(
            r#"
            SELECT COUNT(*) as count
            FROM notifications
            WHERE user_id = $1
            AND deployment_id = $2
            AND is_read = false
            AND is_archived = false
            AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .bind(self.user_id)
        .bind(self.deployment_id)
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(count.0)
    }
}

// =====================================================
// GET SINGLE NOTIFICATION QUERY
// =====================================================
#[derive(Debug)]
pub struct GetNotificationQuery {
    pub notification_id: i64,
    pub user_id: i64,
}

impl Query for GetNotificationQuery {
    type Output = Option<Notification>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = query!(
            r#"
            SELECT
                id, deployment_id, user_id, organization_id, workspace_id,
                title, body, action_url, action_label, severity,
                is_read, read_at, is_archived, archived_at,
                group_id, group_count, dedupe_key,
                source, source_id, metadata,
                created_at, updated_at, expires_at
            FROM notifications
            WHERE id = $1 AND user_id = $2
            "#,
            self.notification_id,
            self.user_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| Notification {
            id: r.id,
            deployment_id: r.deployment_id,
            user_id: r.user_id,
            organization_id: r.organization_id,
            workspace_id: r.workspace_id,
            title: r.title,
            body: r.body,
            action_url: r.action_url,
            action_label: r.action_label,
            severity: match r.severity.as_str() {
                "success" => NotificationSeverity::Success,
                "warning" => NotificationSeverity::Warning,
                "error" => NotificationSeverity::Error,
                _ => NotificationSeverity::Info,
            },
            is_read: r.is_read.unwrap_or(false),
            read_at: r.read_at,
            is_archived: r.is_archived.unwrap_or(false),
            archived_at: r.archived_at,
            group_id: r.group_id,
            group_count: r.group_count.unwrap_or(1),
            dedupe_key: r.dedupe_key,
            source: r.source,
            source_id: r.source_id,
            metadata: r.metadata,
            created_at: r.created_at.unwrap_or_else(|| chrono::Utc::now()),
            updated_at: r.updated_at.unwrap_or_else(|| chrono::Utc::now()),
            expires_at: r.expires_at,
        }))
    }
}

// =====================================================
// CHECK DUPLICATE NOTIFICATION QUERY
// =====================================================
#[derive(Debug)]
pub struct CheckDuplicateNotificationQuery {
    pub deployment_id: i64,
    pub user_id: i64,
    pub dedupe_key: String,
}

impl Query for CheckDuplicateNotificationQuery {
    type Output = bool;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let exists: (bool,) = query_as(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM notifications
                WHERE deployment_id = $1
                AND user_id = $2
                AND dedupe_key = $3
                AND created_at > NOW() - INTERVAL '24 hours'
            )
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.user_id)
        .bind(&self.dedupe_key)
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(exists.0)
    }
}
