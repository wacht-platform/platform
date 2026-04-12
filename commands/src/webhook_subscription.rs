use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::query;

use common::{HasDbRouter, HasRedisProvider, error::AppError};
const SUBSCRIPTION_CACHE_TTL_SECONDS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointWithRules {
    pub id: i64,
    pub url: String,
    pub headers: Option<Value>,
    pub max_retries: i32,
    pub timeout_seconds: i32,
    pub filter_rules: Option<Value>,
    pub signing_secret: String,
}

#[derive(Debug, Deserialize)]
pub struct GetSubscribedEndpointsCommand {
    pub deployment_id: i64,
    pub app_slug: String,
    pub event_name: String,
}

fn subscription_cache_key(deployment_id: i64, app_slug: &str, event_name: &str) -> String {
    format!("webhook:subs:{deployment_id}:{app_slug}:{event_name}")
}

async fn get_cached_endpoints<D>(deps: &D, cache_key: &str) -> Option<Vec<EndpointWithRules>>
where
    D: HasRedisProvider + ?Sized,
{
    let mut redis_conn = deps
        .redis_provider()
        .get_multiplexed_async_connection()
        .await
        .ok()?;
    let cached = redis_conn.get::<_, String>(cache_key).await.ok()?;
    serde_json::from_str::<Vec<EndpointWithRules>>(&cached).ok()
}

async fn cache_endpoints<D>(deps: &D, cache_key: &str, endpoints: &[EndpointWithRules])
where
    D: HasRedisProvider + ?Sized,
{
    if let Ok(json) = serde_json::to_string(endpoints)
        && let Ok(mut redis_conn) = deps
            .redis_provider()
            .get_multiplexed_async_connection()
            .await
    {
        let _: Result<(), _> = redis_conn
            .set_ex(cache_key, json, SUBSCRIPTION_CACHE_TTL_SECONDS)
            .await;
    }
}

impl GetSubscribedEndpointsCommand {
    pub fn new(deployment_id: i64, app_slug: String, event_name: String) -> Self {
        Self {
            deployment_id,
            app_slug,
            event_name,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<EndpointWithRules>, AppError>
    where
        D: HasDbRouter + HasRedisProvider + ?Sized,
    {
        let cache_key =
            subscription_cache_key(self.deployment_id, &self.app_slug, &self.event_name);

        if let Some(endpoints) = get_cached_endpoints(deps, &cache_key).await {
            return Ok(endpoints);
        }

        let endpoints = query!(
            r#"
            SELECT
                e.id as "id!",
                e.url as "url!",
                e.headers,
                e.max_retries as "max_retries!",
                e.timeout_seconds as "timeout_seconds!",
                s.filter_rules,
                a.signing_secret as "signing_secret!"
            FROM webhook_endpoints e
            JOIN webhook_endpoint_subscriptions s ON e.id = s.endpoint_id
            JOIN webhook_apps a ON (e.deployment_id = a.deployment_id AND e.app_slug = a.app_slug)
            WHERE a.app_slug = $1
              AND s.event_name = $2
              AND e.is_active = true
              AND a.is_active = true
              AND a.deployment_id = $3
            "#,
            self.app_slug,
            self.event_name,
            self.deployment_id
        )
        .fetch_all(deps.db_router().writer())
        .await?;

        let endpoints: Vec<EndpointWithRules> = endpoints
            .into_iter()
            .map(|row| EndpointWithRules {
                id: row.id,
                url: row.url,
                headers: row.headers,
                max_retries: row.max_retries,
                timeout_seconds: row.timeout_seconds,
                filter_rules: row.filter_rules,
                signing_secret: row.signing_secret,
            })
            .collect();

        cache_endpoints(deps, &cache_key, &endpoints).await;

        Ok(endpoints)
    }
}

// Helper command to invalidate cache when endpoints change
#[derive(Debug, Deserialize)]
pub struct InvalidateEndpointCacheCommand {
    pub deployment_id: i64,
    pub app_slug: String,
    pub event_names: Vec<String>,
}

impl InvalidateEndpointCacheCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasRedisProvider,
    {
        if let Ok(mut redis_conn) = deps
            .redis_provider()
            .get_multiplexed_async_connection()
            .await
        {
            for event_name in self.event_names {
                let cache_key =
                    subscription_cache_key(self.deployment_id, &self.app_slug, &event_name);
                let _: Result<(), _> = redis_conn.del(&cache_key).await;
            }
        }
        Ok(())
    }
}

// Filter evaluation functions
pub fn evaluate_filter(filter_rules: &Value, payload: &Value) -> bool {
    match filter_rules {
        Value::Object(rules) => {
            for (key, rule) in rules {
                match key.as_str() {
                    "$and" => {
                        if let Some(conditions) = rule.as_array() {
                            for condition in conditions {
                                if !evaluate_filter(condition, payload) {
                                    return false;
                                }
                            }
                        }
                    }
                    "$or" => {
                        if let Some(conditions) = rule.as_array() {
                            let mut any_match = false;
                            for condition in conditions {
                                if evaluate_filter(condition, payload) {
                                    any_match = true;
                                    break;
                                }
                            }
                            if !any_match {
                                return false;
                            }
                        }
                    }
                    field_name => {
                        // Get field value from payload (supports nested paths with dots)
                        let field_value = get_nested_value(payload, field_name);
                        if !evaluate_condition(field_value, rule) {
                            return false;
                        }
                    }
                }
            }
            true
        }
        _ => true, // No filter or invalid filter = pass through
    }
}

fn get_nested_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

fn evaluate_condition(field_value: Option<&Value>, condition: &Value) -> bool {
    // If field doesn't exist and we're not checking for existence, fail
    let field_value = match field_value {
        Some(v) => v,
        None => {
            // Check if condition is checking for non-existence
            if let Some(obj) = condition.as_object() {
                if obj.contains_key("$exists") {
                    return obj["$exists"].as_bool() == Some(false);
                }
            }
            return false;
        }
    };

    // Direct equality check (no operator)
    if !condition.is_object() {
        return field_value == condition;
    }

    // Operator-based conditions
    if let Some(operators) = condition.as_object() {
        for (op, expected) in operators {
            match op.as_str() {
                "$eq" => {
                    if field_value != expected {
                        return false;
                    }
                }
                "$ne" => {
                    if field_value == expected {
                        return false;
                    }
                }
                "$gt" => {
                    if !compare_values(field_value, expected, |a, b| a > b) {
                        return false;
                    }
                }
                "$gte" => {
                    if !compare_values(field_value, expected, |a, b| a >= b) {
                        return false;
                    }
                }
                "$lt" => {
                    if !compare_values(field_value, expected, |a, b| a < b) {
                        return false;
                    }
                }
                "$lte" => {
                    if !compare_values(field_value, expected, |a, b| a <= b) {
                        return false;
                    }
                }
                "$in" => {
                    if let Some(array) = expected.as_array() {
                        if !array.contains(field_value) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                "$nin" => {
                    if let Some(array) = expected.as_array() {
                        if array.contains(field_value) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                "$contains" => {
                    // For arrays: check if array contains value
                    if let Some(array) = field_value.as_array() {
                        if !array.contains(expected) {
                            return false;
                        }
                    }
                    // For strings: check if string contains substring
                    else if let (Some(str_val), Some(substr)) =
                        (field_value.as_str(), expected.as_str())
                    {
                        if !str_val.contains(substr) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                "$exists" => {
                    // Already handled above for None case
                    if expected.as_bool() != Some(true) {
                        return false;
                    }
                }
                _ => {
                    // Unknown operator - ignore for forward compatibility
                }
            }
        }
    }

    true
}

fn compare_values<F>(a: &Value, b: &Value, compare: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    // Try numeric comparison
    if let (Some(a_num), Some(b_num)) = (a.as_f64(), b.as_f64()) {
        return compare(a_num, b_num);
    }

    // Try string comparison
    if let (Some(a_str), Some(b_str)) = (a.as_str(), b.as_str()) {
        return match (compare(0.0, 1.0), compare(1.0, 0.0)) {
            (false, true) => a_str > b_str,   // gt
            (true, true) => a_str >= b_str,   // gte
            (true, false) => a_str < b_str,   // lt
            (false, false) => a_str <= b_str, // lte
        };
    }

    false
}
