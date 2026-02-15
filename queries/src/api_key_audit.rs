use base64::Engine;
use chrono::{DateTime, Utc};
use clickhouse::Row;
use dto::json::api_key::{
    ApiAuditAnalyticsResponse, ApiAuditBlockedReason, ApiAuditLog, ApiAuditLogsResponse,
    ApiAuditRateLimitBreakdown, ApiAuditRateLimitRule, ApiAuditTimeseriesPoint,
    ApiAuditTimeseriesResponse, ApiAuditTopKey, ApiAuditTopPath,
};
use serde::Deserialize;
use serde_json::Value;

use common::error::AppError;
use common::state::AppState;

use super::Query;

#[derive(Debug, Clone)]
enum AuditBind {
    I64(i64),
    String(String),
    DateTime(DateTime<Utc>),
}

fn bind_all(mut query: clickhouse::query::Query, binds: &[AuditBind]) -> clickhouse::query::Query {
    for bind in binds {
        query = match bind {
            AuditBind::I64(v) => query.bind(*v),
            AuditBind::String(v) => query.bind(v.clone()),
            AuditBind::DateTime(v) => query.bind(*v),
        };
    }
    query
}

#[derive(Debug, Clone)]
pub struct GetApiAuditLogsQuery {
    pub deployment_id: i64,
    pub app_slug: String,
    pub limit: u32,
    pub offset: u32,
    pub cursor_ts: Option<DateTime<Utc>>,
    pub cursor_id: Option<String>,
    pub outcome: Option<String>,
    pub key_id: Option<i64>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

impl Query for GetApiAuditLogsQuery {
    type Output = ApiAuditLogsResponse;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.clamp(1, 1000) as usize;
        let (mut where_parts, mut where_binds) = base_where(
            self.deployment_id,
            &self.app_slug,
            self.start_date,
            self.end_date,
            7,
        );

        if let Some(key_id) = self.key_id {
            where_parts.push("key_id = ?".into());
            where_binds.push(AuditBind::I64(key_id));
        }
        if let Some(outcome) = self.outcome.as_deref() {
            match outcome {
                "allowed" => where_parts.push("outcome IN ('allowed','ALLOWED','VALID')".into()),
                "blocked" => {
                    where_parts.push("outcome IN ('blocked','BLOCKED','RATE_LIMITED')".into())
                }
                _ => {}
            }
        }
        if let Some(cursor_ts) = self.cursor_ts {
            if let Some(cursor_id) = self.cursor_id.as_deref() {
                where_parts.push("(timestamp < ? OR (timestamp = ? AND request_id < ?))".into());
                where_binds.push(AuditBind::DateTime(cursor_ts));
                where_binds.push(AuditBind::DateTime(cursor_ts));
                where_binds.push(AuditBind::String(cursor_id.to_string()));
            } else {
                where_parts.push("timestamp < ?".into());
                where_binds.push(AuditBind::DateTime(cursor_ts));
            }
        }

        let mut query = format!(
            "SELECT request_id, deployment_id, app_slug, key_id, key_name, outcome, blocked_by_rule, client_ip, path, user_agent, rate_limits, timestamp \
             FROM api_audit_logs \
             WHERE {} \
             ORDER BY timestamp DESC, request_id DESC \
             LIMIT {}",
            where_parts.join(" AND "),
            limit + 1
        );
        if self.cursor_ts.is_none() {
            query.push_str(&format!(" OFFSET {}", self.offset));
        }

        let rows = bind_all(
            app_state.clickhouse_service.client.query(&query),
            &where_binds,
        )
        .fetch_all::<ApiAuditLogRow>()
        .await?;

        let has_more = rows.len() > limit;
        let mut kept = rows;
        if has_more {
            kept.truncate(limit);
        }

        let data = kept
            .iter()
            .map(|row| ApiAuditLog {
                request_id: row.request_id.clone(),
                deployment_id: row.deployment_id,
                app_slug: row.app_slug.clone(),
                key_id: row.key_id,
                key_name: row.key_name.clone(),
                outcome: row.outcome.clone(),
                blocked_by_rule: row.blocked_by_rule.clone(),
                client_ip: row.client_ip.clone(),
                path: row.path.clone(),
                user_agent: row.user_agent.clone(),
                rate_limits: parse_rate_limits(row.rate_limits.as_deref()),
                timestamp: row.timestamp,
            })
            .collect::<Vec<_>>();

        let next_cursor = kept.last().map(|last| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
                "{}|{}",
                last.timestamp.timestamp_millis(),
                last.request_id
            ))
        });

        Ok(ApiAuditLogsResponse {
            data,
            limit: limit as u32,
            has_more,
            next_cursor,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GetApiAuditAnalyticsQuery {
    pub deployment_id: i64,
    pub app_slug: String,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub key_id: Option<i64>,
    pub include_top_keys: bool,
    pub include_top_paths: bool,
    pub include_blocked_reasons: bool,
    pub include_rate_limits: bool,
    pub top_limit: u32,
}

impl Query for GetApiAuditAnalyticsQuery {
    type Output = ApiAuditAnalyticsResponse;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let top_limit = self.top_limit.clamp(1, 50);
        let (mut where_parts, mut where_binds) = base_where(
            self.deployment_id,
            &self.app_slug,
            self.start_date,
            self.end_date,
            30,
        );
        if let Some(key_id) = self.key_id {
            where_parts.push("key_id = ?".into());
            where_binds.push(AuditBind::I64(key_id));
        }
        let where_clause = where_parts.join(" AND ");

        let stats_query = format!(
            "SELECT count() AS total_requests, \
                    countIf(outcome IN ('allowed','ALLOWED','VALID')) AS allowed_requests, \
                    countIf(outcome IN ('blocked','BLOCKED','RATE_LIMITED')) AS blocked_requests \
             FROM api_audit_logs WHERE {}",
            where_clause
        );

        let stats = bind_all(
            app_state.clickhouse_service.client.query(&stats_query),
            &where_binds,
        )
        .fetch_one::<AuditStatsRow>()
        .await?;

        let keys_used_query = "SELECT countDistinct(key_id) AS count \
             FROM api_audit_logs \
             WHERE deployment_id = ? AND app_slug = ? AND timestamp >= now() - INTERVAL 24 HOUR";
        let keys_used_24h = app_state
            .clickhouse_service
            .client
            .query(keys_used_query)
            .bind(self.deployment_id)
            .bind(self.app_slug.clone())
            .fetch_one::<CountRow>()
            .await?
            .count;

        let top_keys = if self.include_top_keys {
            let query = format!(
                "SELECT key_id, any(key_name) AS key_name, toInt64(count()) AS total_requests \
                 FROM api_audit_logs WHERE {} \
                 GROUP BY key_id ORDER BY total_requests DESC LIMIT {}",
                where_clause, top_limit
            );
            let rows = bind_all(
                app_state.clickhouse_service.client.query(&query),
                &where_binds,
            )
            .fetch_all::<TopKeyRow>()
            .await?;
            Some(
                rows.into_iter()
                    .map(|r| ApiAuditTopKey {
                        key_id: r.key_id,
                        key_name: r.key_name,
                        total_requests: r.total_requests,
                    })
                    .collect(),
            )
        } else {
            None
        };

        let top_paths = if self.include_top_paths {
            let query = format!(
                "SELECT path, toInt64(count()) AS total_requests \
                 FROM api_audit_logs WHERE {} \
                 GROUP BY path ORDER BY total_requests DESC LIMIT {}",
                where_clause, top_limit
            );
            let rows = bind_all(
                app_state.clickhouse_service.client.query(&query),
                &where_binds,
            )
            .fetch_all::<TopPathRow>()
            .await?;
            Some(
                rows.into_iter()
                    .map(|r| ApiAuditTopPath {
                        path: r.path,
                        total_requests: r.total_requests,
                    })
                    .collect(),
            )
        } else {
            None
        };

        let blocked_reasons = if self.include_blocked_reasons {
            let blocked_where = format!(
                "{where_clause} AND outcome IN ('blocked','BLOCKED','RATE_LIMITED') AND blocked_by_rule IS NOT NULL"
            );
            let total_query = format!(
                "SELECT count() AS count FROM api_audit_logs WHERE {}",
                blocked_where
            );
            let total = bind_all(
                app_state.clickhouse_service.client.query(&total_query),
                &where_binds,
            )
            .fetch_one::<CountRow>()
            .await?
            .count as f64;

            let query = format!(
                "SELECT blocked_by_rule, toInt64(count()) AS count \
                 FROM api_audit_logs WHERE {} \
                 GROUP BY blocked_by_rule ORDER BY count DESC LIMIT {}",
                blocked_where, top_limit
            );
            let rows = bind_all(
                app_state.clickhouse_service.client.query(&query),
                &where_binds,
            )
            .fetch_all::<BlockedReasonRow>()
            .await?;
            Some(
                rows.into_iter()
                    .map(|r| ApiAuditBlockedReason {
                        blocked_by_rule: r.blocked_by_rule,
                        count: r.count,
                        percentage: if total > 0.0 {
                            (r.count as f64 / total) * 100.0
                        } else {
                            0.0
                        },
                    })
                    .collect(),
            )
        } else {
            None
        };

        let rate_limit_stats = if self.include_rate_limits {
            let rl_where = format!(
                "{where_clause} AND outcome IN ('blocked','BLOCKED','RATE_LIMITED') AND blocked_by_rule LIKE 'rate_limit:%'"
            );

            let total_query = format!(
                "SELECT count() AS count FROM api_audit_logs WHERE {}",
                rl_where
            );
            let total_hits = bind_all(
                app_state.clickhouse_service.client.query(&total_query),
                &where_binds,
            )
            .fetch_one::<CountRow>()
            .await?
            .count as i64;

            let query = format!(
                "SELECT blocked_by_rule AS rule, toInt64(count()) AS hit_count \
                 FROM api_audit_logs WHERE {} \
                 GROUP BY rule ORDER BY hit_count DESC LIMIT {}",
                rl_where, top_limit
            );
            let rows = bind_all(
                app_state.clickhouse_service.client.query(&query),
                &where_binds,
            )
            .fetch_all::<RateLimitRuleRow>()
            .await?;

            let top_rules = rows
                .into_iter()
                .map(|r| ApiAuditRateLimitRule {
                    rule: r.rule,
                    hit_count: r.hit_count,
                    percentage: if total_hits > 0 {
                        (r.hit_count as f64 / total_hits as f64) * 100.0
                    } else {
                        0.0
                    },
                })
                .collect::<Vec<_>>();

            let percentage_of_blocked = if stats.blocked_requests > 0 {
                (total_hits as f64 / stats.blocked_requests as f64) * 100.0
            } else {
                0.0
            };

            Some(ApiAuditRateLimitBreakdown {
                total_hits,
                percentage_of_blocked,
                top_rules,
            })
        } else {
            None
        };

        let success_rate = if stats.total_requests > 0 {
            (stats.allowed_requests as f64 / stats.total_requests as f64) * 100.0
        } else {
            0.0
        };

        Ok(ApiAuditAnalyticsResponse {
            total_requests: stats.total_requests,
            allowed_requests: stats.allowed_requests,
            blocked_requests: stats.blocked_requests,
            success_rate,
            keys_used_24h,
            top_keys,
            top_paths,
            blocked_reasons,
            rate_limit_stats,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GetApiAuditTimeseriesQuery {
    pub deployment_id: i64,
    pub app_slug: String,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub interval: String,
    pub key_id: Option<i64>,
}

impl Query for GetApiAuditTimeseriesQuery {
    type Output = ApiAuditTimeseriesResponse;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let interval_fn = match self.interval.as_str() {
            "minute" => "toStartOfMinute",
            "day" => "toStartOfDay",
            "week" => "toStartOfWeek",
            "month" => "toStartOfMonth",
            _ => "toStartOfHour",
        };

        let (mut where_parts, mut where_binds) = base_where(
            self.deployment_id,
            &self.app_slug,
            self.start_date,
            self.end_date,
            30,
        );
        if let Some(key_id) = self.key_id {
            where_parts.push("key_id = ?".into());
            where_binds.push(AuditBind::I64(key_id));
        }

        let query = format!(
            "SELECT {}(timestamp) AS bucket, \
                    toInt64(count()) AS total_requests, \
                    toInt64(countIf(outcome IN ('allowed','ALLOWED','VALID'))) AS allowed_requests, \
                    toInt64(countIf(outcome IN ('blocked','BLOCKED','RATE_LIMITED'))) AS blocked_requests \
             FROM api_audit_logs WHERE {} \
             GROUP BY bucket ORDER BY bucket ASC",
            interval_fn,
            where_parts.join(" AND "),
        );

        let rows = bind_all(
            app_state.clickhouse_service.client.query(&query),
            &where_binds,
        )
        .fetch_all::<TimeseriesRow>()
        .await?;

        let data = rows
            .into_iter()
            .map(|row| {
                let success_rate = if row.total_requests > 0 {
                    (row.allowed_requests as f64 / row.total_requests as f64) * 100.0
                } else {
                    0.0
                };
                ApiAuditTimeseriesPoint {
                    timestamp: row.bucket,
                    total_requests: row.total_requests,
                    allowed_requests: row.allowed_requests,
                    blocked_requests: row.blocked_requests,
                    success_rate,
                }
            })
            .collect();

        Ok(ApiAuditTimeseriesResponse {
            data,
            interval: self.interval.clone(),
        })
    }
}

fn base_where(
    deployment_id: i64,
    app_slug: &str,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
    fallback_days: i64,
) -> (Vec<String>, Vec<AuditBind>) {
    let mut where_parts = vec!["deployment_id = ?".into(), "app_slug = ?".into()];
    let mut where_binds = vec![
        AuditBind::I64(deployment_id),
        AuditBind::String(app_slug.to_string()),
    ];

    if let Some(start) = start_date {
        where_parts.push("timestamp >= ?".into());
        where_binds.push(AuditBind::DateTime(start));
        if let Some(end) = end_date {
            where_parts.push("timestamp <= ?".into());
            where_binds.push(AuditBind::DateTime(end));
        }
    } else {
        where_parts.push(format!(
            "timestamp >= now() - INTERVAL {} DAY",
            fallback_days
        ));
    }

    (where_parts, where_binds)
}

fn parse_rate_limits(raw: Option<&str>) -> Option<Value> {
    raw.and_then(|v| serde_json::from_str::<Value>(v).ok())
}

#[derive(Debug, Clone, Row, Deserialize)]
struct ApiAuditLogRow {
    request_id: String,
    deployment_id: i64,
    app_slug: String,
    key_id: i64,
    key_name: String,
    outcome: String,
    blocked_by_rule: Option<String>,
    client_ip: String,
    path: String,
    user_agent: Option<String>,
    rate_limits: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct AuditStatsRow {
    total_requests: u64,
    allowed_requests: u64,
    blocked_requests: u64,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct CountRow {
    count: u64,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct TopKeyRow {
    key_id: i64,
    key_name: String,
    total_requests: i64,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct TopPathRow {
    path: String,
    total_requests: i64,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct BlockedReasonRow {
    blocked_by_rule: String,
    count: i64,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct RateLimitRuleRow {
    rule: String,
    hit_count: i64,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct TimeseriesRow {
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    bucket: DateTime<Utc>,
    total_requests: i64,
    allowed_requests: i64,
    blocked_requests: i64,
}
