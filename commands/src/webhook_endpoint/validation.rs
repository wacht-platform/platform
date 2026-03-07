use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use common::ReadConsistency;
use common::error::AppError;
use queries::GetWebhookEventsQuery;

#[derive(Debug, Deserialize, Clone)]
pub struct EventSubscriptionData {
    pub event_name: String,
    pub filter_rules: Option<Value>,
}

impl From<dto::json::webhook_requests::EventSubscription> for EventSubscriptionData {
    fn from(s: dto::json::webhook_requests::EventSubscription) -> Self {
        Self {
            event_name: s.event_name,
            filter_rules: s.filter_rules,
        }
    }
}

const FILTER_LOGICAL_OPERATORS: [&str; 2] = ["$and", "$or"];
const FILTER_FIELD_OPERATORS: [&str; 10] = [
    "$eq",
    "$ne",
    "$gt",
    "$gte",
    "$lt",
    "$lte",
    "$in",
    "$nin",
    "$contains",
    "$exists",
];
pub(super) const MAX_ENDPOINT_RETRY_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;

fn retry_delay_seconds(attempts: i32) -> i64 {
    match attempts {
        1 => 30,
        2 => 60,
        3 => 5 * 60,
        4 => 15 * 60,
        _ => 6 * 60 * 60,
    }
}

pub(super) fn max_attempts_for_retry_window(max_window_seconds: i64) -> i32 {
    let mut attempts: i32 = 1;
    let mut total_seconds: i64 = 0;

    loop {
        let delay = retry_delay_seconds(attempts);
        if total_seconds + delay > max_window_seconds {
            break;
        }
        total_seconds += delay;
        attempts += 1;
    }

    attempts
}

pub(super) fn validate_endpoint_max_retries(
    max_retries: i32,
    max_allowed: i32,
) -> Result<(), AppError> {
    if max_retries < 1 {
        return Err(AppError::BadRequest(
            "max_retries must be at least 1".to_string(),
        ));
    }
    if max_retries > max_allowed {
        return Err(AppError::BadRequest(format!(
            "max_retries cannot exceed {} (7-day retry window limit)",
            max_allowed
        )));
    }
    Ok(())
}

fn collect_schema_paths(schema: &Value, prefix: Option<&str>, paths: &mut HashSet<String>) {
    let Some(schema_obj) = schema.as_object() else {
        return;
    };

    let Some(properties) = schema_obj.get("properties").and_then(Value::as_object) else {
        return;
    };

    for (field_name, field_schema) in properties {
        let current_path = match prefix {
            Some(parent) if !parent.is_empty() => format!("{}.{}", parent, field_name),
            _ => field_name.clone(),
        };

        paths.insert(current_path.clone());
        collect_schema_paths(field_schema, Some(&current_path), paths);
    }
}

fn validate_filter_condition(condition: &Value, path_ctx: &str) -> Result<(), AppError> {
    let Some(operators) = condition.as_object() else {
        return Ok(());
    };

    for (op, expected) in operators {
        if !FILTER_FIELD_OPERATORS.contains(&op.as_str()) {
            return Err(AppError::BadRequest(format!(
                "Unsupported filter operator '{}' at {}",
                op, path_ctx
            )));
        }

        match op.as_str() {
            "$in" | "$nin" => {
                if !expected.is_array() {
                    return Err(AppError::BadRequest(format!(
                        "Operator '{}' expects an array at {}",
                        op, path_ctx
                    )));
                }
            }
            "$exists" => {
                if !expected.is_boolean() {
                    return Err(AppError::BadRequest(format!(
                        "Operator '$exists' expects a boolean at {}",
                        path_ctx
                    )));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn validate_filter_rules(
    filter_rules: &Value,
    allowed_paths: Option<&HashSet<String>>,
    path_ctx: &str,
) -> Result<(), AppError> {
    let rules = filter_rules.as_object().ok_or_else(|| {
        AppError::BadRequest(format!(
            "Filter rules must be a JSON object for {}",
            path_ctx
        ))
    })?;

    for (key, value) in rules {
        if FILTER_LOGICAL_OPERATORS.contains(&key.as_str()) {
            let conditions = value.as_array().ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Logical operator '{}' expects an array at {}",
                    key, path_ctx
                ))
            })?;

            if conditions.is_empty() {
                return Err(AppError::BadRequest(format!(
                    "Logical operator '{}' cannot be empty at {}",
                    key, path_ctx
                )));
            }

            for (idx, nested) in conditions.iter().enumerate() {
                validate_filter_rules(
                    nested,
                    allowed_paths,
                    &format!("{}.{}[{}]", path_ctx, key, idx),
                )?;
            }
            continue;
        }

        if let Some(paths) = allowed_paths {
            if !paths.contains(key) {
                return Err(AppError::BadRequest(format!(
                    "Unknown filter field '{}' at {}",
                    key, path_ctx
                )));
            }
        }

        validate_filter_condition(value, &format!("{}.{}", path_ctx, key))?;
    }

    Ok(())
}

async fn load_event_schema_map(
    db_router: &common::DbRouter,
    deployment_id: i64,
    app_slug: &str,
) -> Result<HashMap<String, (bool, Option<HashSet<String>>)>, AppError> {
    let reader = db_router.reader(ReadConsistency::Strong);
    let events = GetWebhookEventsQuery::new(deployment_id, app_slug.to_string())
        .execute_with(reader)
        .await?;

    let mut event_map: HashMap<String, (bool, Option<HashSet<String>>)> =
        HashMap::with_capacity(events.len());

    for event in events {
        let allowed_paths = event.schema.as_ref().map(|schema| {
            let mut paths = HashSet::new();
            collect_schema_paths(schema, None, &mut paths);
            paths
        });
        event_map.insert(event.name, (event.is_archived, allowed_paths));
    }

    Ok(event_map)
}

pub(super) async fn validate_event_subscriptions(
    db_router: &common::DbRouter,
    deployment_id: i64,
    app_slug: &str,
    subscriptions: &[EventSubscriptionData],
) -> Result<(), AppError> {
    if subscriptions.is_empty() {
        return Err(AppError::BadRequest(
            "At least one subscription is required".to_string(),
        ));
    }

    let event_map = load_event_schema_map(db_router, deployment_id, app_slug).await?;

    for sub in subscriptions {
        let event_name = sub.event_name.trim();
        if event_name.is_empty() {
            return Err(AppError::BadRequest(
                "Subscription event_name is required".to_string(),
            ));
        }

        let Some((is_archived, allowed_paths)) = event_map.get(event_name) else {
            return Err(AppError::BadRequest(format!(
                "Unknown event '{}' for app '{}'",
                event_name, app_slug
            )));
        };

        if *is_archived {
            return Err(AppError::BadRequest(format!(
                "Event '{}' is archived and cannot be subscribed",
                event_name
            )));
        }

        if let Some(filter_rules) = &sub.filter_rules {
            validate_filter_rules(
                filter_rules,
                allowed_paths.as_ref(),
                &format!("subscriptions.{}", event_name),
            )?;
        }
    }

    Ok(())
}
