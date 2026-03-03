// HTTP handlers for the gateway

use super::GatewayState;
use axum::{
    extract::{ConnectInfo, Json, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use common::tinybird::insert_api_audit_log_async;
use dto::clickhouse::{ApiKeyVerificationEvent, RateLimitState};
use models::api_key::{OAuthScopeDefinition, RateLimit, RateLimitMode};
use queries::{GetGatewayOAuthAccessTokenByHashQuery, Query as QueryTrait};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    time::Instant,
};

use super::validation::{check_permissions, is_valid_api_key_format};

const ALLOWED_HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthzCheckRequest {
    pub principal: AuthzPrincipal,
    pub resource: String,
    pub method: String,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
    pub required_permissions: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalType {
    ApiKey,
    OauthAccessToken,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthzPrincipal {
    #[serde(rename = "type")]
    pub principal_type: PrincipalType,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct AuthzIdentity {
    pub key_id: String,
    pub deployment_id: String,
    pub app_slug: String,
    pub key_name: String,
    pub owner_user_id: Option<String>,
    pub organization_id: Option<String>,
    pub workspace_id: Option<String>,
    pub organization_membership_id: Option<String>,
    pub workspace_membership_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthzCheckResponse {
    pub request_id: String,
    pub allowed: bool,
    pub reason: Option<AuthzDenyReason>,
    pub blocked_rule: Option<String>,
    pub identity: Option<AuthzIdentity>,
    pub permissions: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub rate_limits: Vec<RateLimitState>,
    pub retry_after: Option<u32>,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthzDenyReason {
    PermissionDenied,
    RateLimited,
}

fn safe_header_value(s: String) -> HeaderValue {
    HeaderValue::from_str(&s).unwrap_or_else(|_| HeaderValue::from_static("invalid"))
}

fn header_map_to_string_map(headers: &HeaderMap) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (name, value) in headers {
        if let Ok(v) = value.to_str() {
            out.insert(name.as_str().to_string(), v.to_string());
        }
    }
    out
}

fn normalize_resource_path(resource: &str) -> Option<String> {
    let mut normalized = resource.trim();
    if normalized.is_empty() {
        return None;
    }

    if let Some(idx) = normalized.find('#') {
        normalized = &normalized[..idx];
    }
    if let Some(idx) = normalized.find('?') {
        normalized = &normalized[..idx];
    }

    let normalized = normalized.trim();
    if normalized.is_empty() {
        return None;
    }

    if normalized.starts_with('/') {
        Some(normalized.to_string())
    } else {
        Some(format!("/{}", normalized))
    }
}

fn resource_type(resource: &str) -> &'static str {
    if resource.starts_with("urn:wacht:organization:") {
        "organization"
    } else if resource.starts_with("urn:wacht:workspace:") {
        "workspace"
    } else if resource.starts_with("urn:wacht:user:") {
        "personal"
    } else {
        ""
    }
}

fn required_expr_from_permissions(required_permissions: Option<Vec<String>>) -> Option<String> {
    required_permissions.and_then(|perms| {
        let cleaned: Vec<String> = perms
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();

        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.join(" AND "))
        }
    })
}

fn derive_effective_permissions_from_oauth(
    scopes: &[String],
    scope_definitions: &[OAuthScopeDefinition],
    resource: Option<&str>,
) -> Vec<String> {
    let mut effective: HashSet<String> = scopes.iter().cloned().collect();
    let rtype = resource.map(resource_type).unwrap_or("");
    if rtype.is_empty() {
        let mut out: Vec<String> = effective.into_iter().collect();
        out.sort();
        return out;
    }

    for def in scope_definitions {
        if !scopes.iter().any(|scope| scope == &def.scope) {
            continue;
        }
        let category = def.category.trim().to_ascii_lowercase();
        if category != rtype {
            continue;
        }
        let mapped_permission = match rtype {
            "organization" => def.organization_permission.as_deref(),
            "workspace" => def.workspace_permission.as_deref(),
            _ => None,
        };
        if let Some(permission) = mapped_permission.map(str::trim).filter(|v| !v.is_empty()) {
            effective.insert(permission.to_string());
        }
    }

    let mut out: Vec<String> = effective.into_iter().collect();
    out.sort();
    out
}

fn error_response(status: StatusCode, request_id: String, reason: &str) -> Response {
    let _ = reason;
    (
        status,
        Json(AuthzCheckResponse {
            request_id,
            allowed: false,
            reason: None,
            blocked_rule: None,
            identity: None,
            permissions: vec![],
            metadata: None,
            rate_limits: vec![],
            retry_after: None,
            headers: HashMap::new(),
        }),
    )
        .into_response()
}

pub async fn check_authz(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<GatewayState>,
    Json(payload): Json<AuthzCheckRequest>,
) -> Response {
    let start_time = Instant::now();
    let request_id = state
        .app_state
        .sf
        .next_id()
        .map(|id| format!("req_{}", id))
        .unwrap_or_else(|_| "req_unknown".to_string());
    let limiter = &state.rate_limiter;
    let app_state = &state.app_state;

    let method = payload.method.trim().to_ascii_uppercase();
    if !ALLOWED_HTTP_METHODS.contains(&method.as_str()) {
        return error_response(StatusCode::BAD_REQUEST, request_id, "invalid_method");
    }

    let resource = match normalize_resource_path(&payload.resource) {
        Some(v) => v,
        None => return error_response(StatusCode::BAD_REQUEST, request_id, "invalid_resource"),
    };

    if matches!(
        payload.principal.principal_type,
        PrincipalType::OauthAccessToken
    ) {
        return check_authz_oauth_access_token(
            addr, headers, state, payload, request_id, resource, method, start_time,
        )
        .await;
    }

    let api_key = payload.principal.value.trim();
    if api_key.is_empty() {
        return error_response(StatusCode::UNAUTHORIZED, request_id, "api_key_required");
    }

    if !is_valid_api_key_format(api_key) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            request_id,
            "invalid_api_key_format",
        );
    }

    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    let key_data = match limiter.get_api_key_data(key_hash, app_state).await {
        Ok(data) => data,
        Err(response) => return response,
    };

    if let Some(expires_at) = key_data.expires_at {
        if expires_at < Utc::now() {
            return error_response(StatusCode::UNAUTHORIZED, request_id, "api_key_expired");
        }
    }

    let user_agent = payload
        .user_agent
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("x-original-user-agent")
                .or_else(|| headers.get("user-agent"))
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    let client_ip = payload
        .client_ip
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("x-original-client-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| addr.ip().to_string());

    let mut response_headers = HeaderMap::new();

    let mut effective_permissions: std::collections::HashSet<String> =
        key_data.permissions.iter().cloned().collect();
    for perm in key_data
        .org_role_permissions
        .iter()
        .chain(key_data.workspace_role_permissions.iter())
    {
        effective_permissions.insert(perm.clone());
    }
    let mut effective_permissions: Vec<String> = effective_permissions.into_iter().collect();
    effective_permissions.sort();

    response_headers.insert(
        "X-Wacht-Key-ID",
        safe_header_value(key_data.key_id.to_string()),
    );
    response_headers.insert(
        "X-Wacht-Deployment-ID",
        safe_header_value(key_data.deployment_id.to_string()),
    );
    response_headers.insert(
        "X-Wacht-App-Slug",
        safe_header_value(key_data.app_slug.clone()),
    );
    response_headers.insert(
        "X-Wacht-Key-Name",
        safe_header_value(key_data.key_name.clone()),
    );

    if let Some(org_id) = key_data.organization_id {
        response_headers.insert(
            "X-Wacht-Organization-ID",
            safe_header_value(org_id.to_string()),
        );
    }
    if let Some(workspace_id) = key_data.workspace_id {
        response_headers.insert(
            "X-Wacht-Workspace-ID",
            safe_header_value(workspace_id.to_string()),
        );
    }
    if let Some(org_membership_id) = key_data.organization_membership_id {
        response_headers.insert(
            "X-Wacht-Organization-Membership-ID",
            safe_header_value(org_membership_id.to_string()),
        );
    }
    if let Some(workspace_membership_id) = key_data.workspace_membership_id {
        response_headers.insert(
            "X-Wacht-Workspace-Membership-ID",
            safe_header_value(workspace_membership_id.to_string()),
        );
    }

    if !effective_permissions.is_empty() {
        let permissions_str = effective_permissions.join(",");
        if let Ok(header_value) = HeaderValue::from_str(&permissions_str) {
            response_headers.insert("X-Wacht-Permissions", header_value);
        }
    }

    if let Some(metadata_obj) = key_data.metadata.as_object() {
        if !metadata_obj.is_empty() {
            let metadata_pairs: Vec<String> = metadata_obj
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|v_str| format!("{}={}", k, v_str)))
                .collect();
            if !metadata_pairs.is_empty() {
                let metadata_str = metadata_pairs.join(",");
                if let Ok(header_value) = HeaderValue::from_str(&metadata_str) {
                    response_headers.insert("X-Wacht-Metadata", header_value);
                }
            }
        }
    }

    let required_expr = payload.required_permissions.and_then(|perms| {
        let cleaned: Vec<String> = perms
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();

        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.join(" AND "))
        }
    });

    let mut all_allowed = true;
    let mut min_retry_after: Option<u32> = None;
    let mut blocked_by_rule: Option<String> = None;
    let mut rate_limit_states: Vec<RateLimitState> = Vec::new();

    if let Some(expr) = required_expr {
        let key_permissions: std::collections::HashSet<&str> =
            effective_permissions.iter().map(|s| s.as_str()).collect();
        if !check_permissions(&key_permissions, &expr) {
            all_allowed = false;
            blocked_by_rule = Some("permission_denied".to_string());
            response_headers.insert(
                "X-Wacht-Permission-Denied",
                safe_header_value("true".to_string()),
            );
        }
    }

    if all_allowed {
        let effective_limits = if let Some(scheme_slug) = &key_data.rate_limit_scheme_slug {
            if let Some(scheme_rules) = limiter
                .get_rate_limit_scheme(key_data.deployment_id, scheme_slug.clone(), app_state)
                .await
            {
                let mut matching_rules: Vec<&RateLimit> = scheme_rules
                    .iter()
                    .filter(|rule| rule.matches_endpoint(&resource))
                    .collect();
                matching_rules.sort_by_key(|r| r.priority);
                matching_rules.into_iter().cloned().collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        let mut limits = Vec::new();
        for rate_limit in &effective_limits {
            let window_ms = match rate_limit.unit {
                models::api_key::RateLimitUnit::Millisecond => rate_limit.duration as i64,
                models::api_key::RateLimitUnit::Second => rate_limit.duration as i64 * 1000,
                models::api_key::RateLimitUnit::Minute => rate_limit.duration as i64 * 60_000,
                models::api_key::RateLimitUnit::Hour => rate_limit.duration as i64 * 3_600_000,
                models::api_key::RateLimitUnit::Day => rate_limit.duration as i64 * 86_400_000,
                models::api_key::RateLimitUnit::CalendarDay => {
                    rate_limit.duration as i64 * 86_400_000
                }
                models::api_key::RateLimitUnit::Month => {
                    rate_limit.duration as i64 * 30 * 86_400_000
                }
                models::api_key::RateLimitUnit::CalendarMonth => {
                    rate_limit.duration as i64 * 30 * 86_400_000
                }
            };

            let rate_limit_mode = rate_limit.effective_mode();
            let is_calendar_day = matches!(
                rate_limit.unit,
                models::api_key::RateLimitUnit::CalendarDay
                    | models::api_key::RateLimitUnit::CalendarMonth
            );
            limits.push((
                rate_limit.max_requests as u32,
                window_ms,
                rate_limit_mode,
                is_calendar_day,
            ));
        }

        for (limit, window_ms, rate_limit_mode, is_calendar_day) in limits.iter() {
            let key = match rate_limit_mode {
                RateLimitMode::PerApp => {
                    format!(
                        "app:{}:{}:{}",
                        key_data.deployment_id, key_data.app_slug, window_ms
                    )
                }
                RateLimitMode::PerKey => {
                    format!("key:{}:{}:{}", key_data.key_id, resource, window_ms)
                }
                RateLimitMode::PerKeyAndIp => {
                    format!(
                        "key_ip:{}:{}:{}:{}",
                        key_data.key_id, client_ip, resource, window_ms
                    )
                }
                RateLimitMode::PerAppAndIp => {
                    format!(
                        "app_ip:{}:{}:{}:{}",
                        key_data.deployment_id, key_data.app_slug, client_ip, window_ms
                    )
                }
            };

            let (allowed, remaining, retry_after) = limiter
                .check_rate_limit(key, *limit, *window_ms, *is_calendar_day)
                .await;

            let window_sec = if *window_ms < 1000 {
                format!("{}ms", window_ms)
            } else {
                format!("{}s", window_ms / 1000)
            };
            let limit_header = format!("X-RateLimit-{}-Limit", window_sec);
            let remaining_header = format!("X-RateLimit-{}-Remaining", window_sec);
            response_headers.insert(
                axum::http::HeaderName::from_bytes(limit_header.as_bytes()).unwrap(),
                safe_header_value(limit.to_string()),
            );
            response_headers.insert(
                axum::http::HeaderName::from_bytes(remaining_header.as_bytes()).unwrap(),
                safe_header_value(remaining.to_string()),
            );

            if !allowed {
                all_allowed = false;
                min_retry_after =
                    Some(min_retry_after.map_or(retry_after, |cur| cur.min(retry_after)));
                let reset_header = format!("X-RateLimit-{}-Reset", window_sec);
                response_headers.insert(
                    axum::http::HeaderName::from_bytes(reset_header.as_bytes()).unwrap(),
                    safe_header_value(retry_after.to_string()),
                );
                if blocked_by_rule.is_none() {
                    blocked_by_rule = Some(format!("{}/{}", limit, window_sec));
                }
            }

            rate_limit_states.push(RateLimitState {
                rule: format!("{}/{}", limit, window_sec),
                remaining: remaining as i32,
                limit: *limit as i32,
            });
        }
    }

    let latency_us = start_time.elapsed().as_micros() as i64;
    let outcome = if all_allowed { "VALID" } else { "BLOCKED" };

    let mut audit_event = ApiKeyVerificationEvent::new(
        request_id.clone(),
        key_data.deployment_id,
        key_data.app_slug.clone(),
        key_data.key_id,
        key_data.key_name.clone(),
        outcome.to_string(),
        client_ip,
        format!("{} {}", method, resource),
        user_agent,
    )
    .with_rate_limits(rate_limit_states.clone())
    .with_latency(latency_us);

    if let Some(rule) = blocked_by_rule.clone() {
        audit_event = audit_event.with_blocked_by(rule);
    }

    insert_api_audit_log_async(audit_event);

    let reason = if all_allowed {
        None
    } else if blocked_by_rule.as_deref() == Some("permission_denied") {
        Some(AuthzDenyReason::PermissionDenied)
    } else {
        Some(AuthzDenyReason::RateLimited)
    };
    let blocked_rule = blocked_by_rule.clone();

    (
        StatusCode::OK,
        Json(AuthzCheckResponse {
            request_id,
            allowed: all_allowed,
            reason,
            blocked_rule,
            identity: Some(AuthzIdentity {
                key_id: key_data.key_id.to_string(),
                deployment_id: key_data.deployment_id.to_string(),
                app_slug: key_data.app_slug,
                key_name: key_data.key_name,
                owner_user_id: key_data.owner_user_id.map(|v| v.to_string()),
                organization_id: key_data.organization_id.map(|v| v.to_string()),
                workspace_id: key_data.workspace_id.map(|v| v.to_string()),
                organization_membership_id: key_data
                    .organization_membership_id
                    .map(|v| v.to_string()),
                workspace_membership_id: key_data.workspace_membership_id.map(|v| v.to_string()),
            }),
            permissions: effective_permissions,
            metadata: Some(key_data.metadata),
            rate_limits: rate_limit_states,
            retry_after: min_retry_after,
            headers: header_map_to_string_map(&response_headers),
        }),
    )
        .into_response()
}

async fn check_authz_oauth_access_token(
    addr: SocketAddr,
    headers: HeaderMap,
    state: GatewayState,
    payload: AuthzCheckRequest,
    request_id: String,
    resource: String,
    method: String,
    start_time: Instant,
) -> Response {
    let limiter = &state.rate_limiter;
    let app_state = &state.app_state;
    let token = payload.principal.value.trim();
    if token.is_empty() {
        return error_response(
            StatusCode::UNAUTHORIZED,
            request_id,
            "access_token_required",
        );
    }

    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let token_hash = format!("{:x}", hasher.finalize());

    let token_data = match GetGatewayOAuthAccessTokenByHashQuery::new(token_hash.clone())
        .execute(app_state)
        .await
    {
        Ok(Some(data)) => data,
        Ok(None) => return error_response(StatusCode::UNAUTHORIZED, request_id, "invalid_token"),
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                "database_error",
            );
        }
    };

    if !token_data.active {
        return error_response(StatusCode::UNAUTHORIZED, request_id, "invalid_token");
    }

    let user_agent = payload
        .user_agent
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("x-original-user-agent")
                .or_else(|| headers.get("user-agent"))
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    let client_ip = payload
        .client_ip
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("x-original-client-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| addr.ip().to_string());

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        "X-Wacht-Principal-Type",
        safe_header_value("oauth_access_token".to_string()),
    );
    response_headers.insert(
        "X-Wacht-Deployment-ID",
        safe_header_value(token_data.deployment_id.to_string()),
    );
    response_headers.insert(
        "X-Wacht-App-Slug",
        safe_header_value(token_data.app_slug.clone()),
    );
    response_headers.insert(
        "X-Wacht-OAuth-Client-ID",
        safe_header_value(token_data.client_id.clone()),
    );
    if let Some(granted_resource_urn) = token_data.granted_resource.clone() {
        response_headers.insert(
            "X-Wacht-Granted-Resource",
            safe_header_value(granted_resource_urn),
        );
    }
    if let Some(requested_resource_urn) = token_data.resource.clone() {
        response_headers.insert(
            "X-Wacht-Requested-Resource",
            safe_header_value(requested_resource_urn),
        );
    }
    if !token_data.scopes.is_empty() {
        response_headers.insert(
            "X-Wacht-Scopes",
            safe_header_value(token_data.scopes.join(",")),
        );
    }

    let effective_permissions = derive_effective_permissions_from_oauth(
        &token_data.scopes,
        &token_data.scope_definitions,
        token_data.granted_resource.as_deref(),
    );

    if !effective_permissions.is_empty() {
        let permissions_str = effective_permissions.join(",");
        if let Ok(header_value) = HeaderValue::from_str(&permissions_str) {
            response_headers.insert("X-Wacht-Permissions", header_value);
        }
    }

    let required_expr = required_expr_from_permissions(payload.required_permissions);

    let mut all_allowed = true;
    let mut min_retry_after: Option<u32> = None;
    let mut blocked_by_rule: Option<String> = None;
    let mut rate_limit_states: Vec<RateLimitState> = Vec::new();

    if let Some(expr) = required_expr {
        let key_permissions: std::collections::HashSet<&str> =
            effective_permissions.iter().map(|s| s.as_str()).collect();
        if !check_permissions(&key_permissions, &expr) {
            all_allowed = false;
            blocked_by_rule = Some("permission_denied".to_string());
            response_headers.insert(
                "X-Wacht-Permission-Denied",
                safe_header_value("true".to_string()),
            );
        }
    }

    if all_allowed {
        let effective_limits = if let Some(scheme_slug) = &token_data.rate_limit_scheme_slug {
            if let Some(scheme_rules) = limiter
                .get_rate_limit_scheme(token_data.deployment_id, scheme_slug.clone(), app_state)
                .await
            {
                let mut matching_rules: Vec<&RateLimit> = scheme_rules
                    .iter()
                    .filter(|rule| rule.matches_endpoint(&resource))
                    .collect();
                matching_rules.sort_by_key(|r| r.priority);
                matching_rules.into_iter().cloned().collect()
            } else {
                token_data.rate_limits.clone()
            }
        } else {
            token_data.rate_limits.clone()
        };

        let mut limits = Vec::new();
        for rate_limit in &effective_limits {
            let window_ms = match rate_limit.unit {
                models::api_key::RateLimitUnit::Millisecond => rate_limit.duration as i64,
                models::api_key::RateLimitUnit::Second => rate_limit.duration as i64 * 1000,
                models::api_key::RateLimitUnit::Minute => rate_limit.duration as i64 * 60_000,
                models::api_key::RateLimitUnit::Hour => rate_limit.duration as i64 * 3_600_000,
                models::api_key::RateLimitUnit::Day => rate_limit.duration as i64 * 86_400_000,
                models::api_key::RateLimitUnit::CalendarDay => {
                    rate_limit.duration as i64 * 86_400_000
                }
                models::api_key::RateLimitUnit::Month => {
                    rate_limit.duration as i64 * 30 * 86_400_000
                }
                models::api_key::RateLimitUnit::CalendarMonth => {
                    rate_limit.duration as i64 * 30 * 86_400_000
                }
            };

            let rate_limit_mode = rate_limit.effective_mode();
            let is_calendar_day = matches!(
                rate_limit.unit,
                models::api_key::RateLimitUnit::CalendarDay
                    | models::api_key::RateLimitUnit::CalendarMonth
            );
            limits.push((
                rate_limit.max_requests as u32,
                window_ms,
                rate_limit_mode,
                is_calendar_day,
            ));
        }

        for (limit, window_ms, rate_limit_mode, is_calendar_day) in limits.iter() {
            let oauth_principal_id = token_data
                .oauth_grant_id
                .map(|id| format!("grant:{}", id))
                .unwrap_or_else(|| format!("token:{}", token_hash));
            let key = match rate_limit_mode {
                RateLimitMode::PerApp => {
                    format!(
                        "app:{}:{}:{}",
                        token_data.deployment_id, token_data.app_slug, window_ms
                    )
                }
                RateLimitMode::PerKey => {
                    format!("oauth:{}:{}:{}", oauth_principal_id, resource, window_ms)
                }
                RateLimitMode::PerKeyAndIp => {
                    format!(
                        "oauth_ip:{}:{}:{}:{}",
                        oauth_principal_id, client_ip, resource, window_ms
                    )
                }
                RateLimitMode::PerAppAndIp => {
                    format!(
                        "app_ip:{}:{}:{}:{}",
                        token_data.deployment_id, token_data.app_slug, client_ip, window_ms
                    )
                }
            };

            let (allowed, remaining, retry_after) = limiter
                .check_rate_limit(key, *limit, *window_ms, *is_calendar_day)
                .await;

            let window_sec = if *window_ms < 1000 {
                format!("{}ms", window_ms)
            } else {
                format!("{}s", window_ms / 1000)
            };
            let limit_header = format!("X-RateLimit-{}-Limit", window_sec);
            let remaining_header = format!("X-RateLimit-{}-Remaining", window_sec);
            response_headers.insert(
                axum::http::HeaderName::from_bytes(limit_header.as_bytes()).unwrap(),
                safe_header_value(limit.to_string()),
            );
            response_headers.insert(
                axum::http::HeaderName::from_bytes(remaining_header.as_bytes()).unwrap(),
                safe_header_value(remaining.to_string()),
            );

            if !allowed {
                all_allowed = false;
                min_retry_after =
                    Some(min_retry_after.map_or(retry_after, |cur| cur.min(retry_after)));
                let reset_header = format!("X-RateLimit-{}-Reset", window_sec);
                response_headers.insert(
                    axum::http::HeaderName::from_bytes(reset_header.as_bytes()).unwrap(),
                    safe_header_value(retry_after.to_string()),
                );
                if blocked_by_rule.is_none() {
                    blocked_by_rule = Some(format!("{}/{}", limit, window_sec));
                }
            }

            rate_limit_states.push(RateLimitState {
                rule: format!("{}/{}", limit, window_sec),
                remaining: remaining as i32,
                limit: *limit as i32,
            });
        }
    }

    let latency_us = start_time.elapsed().as_micros() as i64;
    let outcome = if all_allowed { "VALID" } else { "BLOCKED" };
    let mut audit_event = ApiKeyVerificationEvent::new(
        request_id.clone(),
        token_data.deployment_id,
        token_data.app_slug.clone(),
        0,
        token_data.client_id.clone(),
        outcome.to_string(),
        client_ip,
        format!("{} {}", method, resource),
        user_agent,
    )
    .with_rate_limits(rate_limit_states.clone())
    .with_latency(latency_us);
    if let Some(rule) = blocked_by_rule.clone() {
        audit_event = audit_event.with_blocked_by(rule);
    }
    insert_api_audit_log_async(audit_event);

    let reason = if all_allowed {
        None
    } else if blocked_by_rule.as_deref() == Some("permission_denied") {
        Some(AuthzDenyReason::PermissionDenied)
    } else {
        Some(AuthzDenyReason::RateLimited)
    };

    (
        StatusCode::OK,
        Json(AuthzCheckResponse {
            request_id,
            allowed: all_allowed,
            reason,
            blocked_rule: blocked_by_rule,
            identity: Some(AuthzIdentity {
                key_id: token_data.oauth_client_id.to_string(),
                deployment_id: token_data.deployment_id.to_string(),
                app_slug: token_data.app_slug,
                key_name: token_data.client_id,
                owner_user_id: token_data.owner_user_id.map(|v| v.to_string()),
                organization_id: None,
                workspace_id: None,
                organization_membership_id: None,
                workspace_membership_id: None,
            }),
            permissions: effective_permissions,
            metadata: Some(serde_json::json!({
                "principal_type": "oauth_access_token",
                "scopes": token_data.scopes,
                "resource": token_data.resource,
                "granted_resource": token_data.granted_resource,
                "expires_at": token_data.expires_at,
            })),
            rate_limits: rate_limit_states,
            retry_after: min_retry_after,
            headers: header_map_to_string_map(&response_headers),
        }),
    )
        .into_response()
}

/// Health check endpoint
pub async fn health() -> &'static str {
    "OK"
}
