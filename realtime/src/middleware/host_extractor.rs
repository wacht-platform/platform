use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use common::utils::validation::HostValidator;
use serde_json::json;
use tracing::warn;

/// Extracted host information from the HTTP Host header
#[derive(Debug, Clone)]
pub struct ExtractedHost(pub String);

/// Middleware that extracts and validates the Host header from incoming requests
/// 
/// This middleware:
/// 1. Extracts the host from the HTTP Host header
/// 2. Validates that the host is not an IP address (like the frontend API)
/// 3. Makes the validated host available to downstream handlers via Extension
/// 4. Returns 400/404 responses for invalid hosts
pub struct HostExtractorMiddleware;

impl HostExtractorMiddleware {
    /// Creates the middleware function for use with Axum
    pub async fn extract_host(mut request: Request, next: Next) -> Result<Response, Response> {
        let headers = request.headers();
        
        // Extract host from Host header
        let host = match extract_host_from_headers(headers) {
            Some(host) => host,
            None => {
                warn!("WebSocket connection attempted without Host header");
                return Err(create_error_response(
                    StatusCode::BAD_REQUEST,
                    "Missing Host header",
                ));
            }
        };

        // Validate that host is not an IP address
        if !HostValidator::is_valid_host(&host) {
            warn!("WebSocket connection attempted with IP address host: {}", host);
            return Err(create_error_response(
                StatusCode::NOT_FOUND,
                "Deployment not found",
            ));
        }

        // Add the validated host to the request extensions
        request.extensions_mut().insert(ExtractedHost(host));

        // Continue to the next middleware/handler
        Ok(next.run(request).await)
    }
}

/// Extracts host from HTTP headers, handling edge cases
fn extract_host_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .map(|s| {
            // Remove port if present (e.g., "example.com:3000" -> "example.com")
            s.split(':').next().unwrap_or(s).to_string()
        })
}

/// Creates a consistent error response for host validation failures
fn create_error_response(status: StatusCode, message: &str) -> Response {
    let body = json!({
        "message": message
    });
    
    axum::response::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
        .into_response()
}