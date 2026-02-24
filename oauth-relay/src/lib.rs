use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tower_service::Service;
use url::Url;
use worker::*;

#[derive(Debug, Deserialize)]
struct OAuthParams {
    /// OAuth authorization code
    code: Option<String>,
    /// OAuth state parameter (contains HMAC-signed data with redirect URI)
    state: Option<String>,
    /// OAuth error code
    error: Option<String>,
    /// OAuth error description
    error_description: Option<String>,
}

fn router() -> Router {
    Router::new()
        .route("/", get(handle_oauth_callback))
        .route("/health", get(health_check))
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    _env: Env,
    _ctx: Context,
) -> Result<axum::http::Response<axum::body::Body>> {
    console_error_panic_hook::set_once();
    Ok(router().call(req).await?)
}

/// Health check endpoint
async fn health_check() -> &'static str {
    "OK"
}

/// Handle OAuth callback and redirect to the target host
async fn handle_oauth_callback(Query(params): Query<OAuthParams>) -> Response {
    let state = match params.state.as_ref() {
        Some(s) if !s.is_empty() => s,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing or empty 'state' parameter. This service requires a state parameter with redirect information.",
            )
                .into_response();
        }
    };

    let parts_state: Vec<&str> = state.split('.').collect();
    if parts_state.len() != 3 {
        return (
            StatusCode::BAD_REQUEST,
            "Invalid state format for relay verification",
        )
            .into_response();
    }

    let state_data = parts_state[0];
    let relay_sig = parts_state[2];

    let decoded_bytes = match URL_SAFE_NO_PAD.decode(state_data) {
        Ok(bytes) => bytes,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Failed to decode state parameter").into_response();
        }
    };

    let decoded_str = match String::from_utf8(decoded_bytes) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid state encoding").into_response();
        }
    };

    if !verify_relay_state_signature(&decoded_str, relay_sig) {
        return (StatusCode::BAD_REQUEST, "Invalid relay state signature").into_response();
    }

    let parts: Vec<&str> = decoded_str.split('|').collect();

    let frontend_host = if parts.is_empty() {
        return (StatusCode::BAD_REQUEST, "Invalid state data format").into_response();
    } else if parts[0] == "sign_in" && parts.len() >= 5 {
        parts[4]
    } else if parts[0] == "connect_social" && parts.len() >= 7 {
        parts[6]
    } else {
        return (
            StatusCode::BAD_REQUEST,
            "Unable to extract frontend host from state",
        )
            .into_response();
    };

    let redirect_uri = format!("https://{}/sso-callback", frontend_host);

    let target_url = match Url::parse(&redirect_uri) {
        Ok(url) => url,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid redirect URI: {}", redirect_uri),
            )
                .into_response();
        }
    };

    if target_url.scheme() != "https" {
        let is_localhost = target_url
            .host_str()
            .map(|h| h == "localhost" || h == "127.0.0.1" || h == "::1")
            .unwrap_or(false);

        if !is_localhost {
            return (
                StatusCode::BAD_REQUEST,
                "Target host must use HTTPS for security",
            )
                .into_response();
        }
    }

    let mut redirect_url = target_url;
    redirect_url.set_query(None);
    let mut query_pairs = vec![];

    if let Some(ref code) = params.code {
        query_pairs.push(format!("code={}", urlencoding::encode(code)));
    }

    if let Some(ref state) = params.state {
        query_pairs.push(format!("state={}", urlencoding::encode(state)));
    }

    if let Some(ref error) = params.error {
        query_pairs.push(format!("error={}", urlencoding::encode(error)));
    }

    if let Some(ref error_desc) = params.error_description {
        query_pairs.push(format!(
            "error_description={}",
            urlencoding::encode(error_desc)
        ));
    }

    if !query_pairs.is_empty() {
        redirect_url.set_query(Some(&query_pairs.join("&")));
    }

    console_log!("OAuth relay: callback redirected");

    Redirect::temporary(redirect_url.as_str()).into_response()
}

fn relay_state_secret() -> Option<Vec<u8>> {
    let encryption_key = std::env::var("ENCRYPTION_KEY").ok()?;
    let key = encryption_key.trim();
    if key.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(b"ors1:");
    hasher.update(key.as_bytes());
    Some(hasher.finalize().to_vec())
}

fn verify_relay_state_signature(payload: &str, provided_signature: &str) -> bool {
    if provided_signature.trim().is_empty() {
        return false;
    }
    let Some(secret) = relay_state_secret() else {
        return false;
    };
    let Ok(provided_sig_bytes) = URL_SAFE_NO_PAD.decode(provided_signature) else {
        return false;
    };
    type HmacSha256 = Hmac<Sha256>;
    let Ok(mut mac) = HmacSha256::new_from_slice(&secret) else {
        return false;
    };
    mac.update(payload.as_bytes());
    mac.verify_slice(&provided_sig_bytes).is_ok()
}
