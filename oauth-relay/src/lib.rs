use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
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
    // Extract the redirect URI from the state parameter
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

    // The state format is: base64_data.hmac_signature
    // We only need the data part to extract the redirect URI
    let state_data = match state.split('.').next() {
        Some(data) => data,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Invalid state format",
            )
                .into_response();
        }
    };

    // Decode the base64 state data
    let decoded_bytes = match URL_SAFE_NO_PAD.decode(state_data) {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "Failed to decode state parameter",
            )
                .into_response();
        }
    };

    let decoded_str = match String::from_utf8(decoded_bytes) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "Invalid state encoding",
            )
                .into_response();
        }
    };

    // Parse the pipe-delimited state data
    // Format for sign_in: action|attempt_id|redirect_uri|timestamp
    // Format for connect_social: action|user_id|session_id|provider|redirect_uri|timestamp
    let parts: Vec<&str> = decoded_str.split('|').collect();
    
    // Extract redirect URI based on action type
    let redirect_uri = if parts.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "Invalid state data format",
        )
            .into_response();
    } else if parts[0] == "sign_in" && parts.len() >= 4 {
        // For sign_in: redirect_uri is at index 2
        parts[2]
    } else if parts[0] == "connect_social" && parts.len() >= 6 {
        // For connect_social: redirect_uri is at index 4
        parts[4]
    } else {
        return (
            StatusCode::BAD_REQUEST,
            "Unable to extract redirect URI from state",
        )
            .into_response();
    };

    // Validate that the redirect URI is a valid URL
    let target_url = match Url::parse(redirect_uri) {
        Ok(url) => url,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid redirect URI: {}", redirect_uri),
            )
                .into_response();
        }
    };

    // Security check: Ensure the target uses HTTPS (allow HTTP only for localhost)
    if target_url.scheme() != "https" {
        let is_localhost = target_url.host_str()
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

    // Build the redirect URL with OAuth parameters
    let mut redirect_url = target_url;
    
    // Clear any existing query parameters from the host URL
    redirect_url.set_query(None);
    
    // Add OAuth parameters to the redirect URL
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
        query_pairs.push(format!("error_description={}", urlencoding::encode(error_desc)));
    }
    
    if !query_pairs.is_empty() {
        redirect_url.set_query(Some(&query_pairs.join("&")));
    }

    // Log the redirect for debugging (in production, this goes to Cloudflare logs)
    console_log!("OAuth relay: Redirecting to {}", redirect_url);

    // Perform the redirect
    Redirect::temporary(redirect_url.as_str()).into_response()
}