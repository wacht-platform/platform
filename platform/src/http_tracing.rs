use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, MatchedPath};
use axum::http::{Request, Response, Version};
use std::net::SocketAddr;
use std::time::Duration;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::trace::TraceLayer;
use tracing::Span;

fn http_version(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "0.9",
        Version::HTTP_10 => "1.0",
        Version::HTTP_11 => "1.1",
        Version::HTTP_2 => "2.0",
        Version::HTTP_3 => "3.0",
        _ => "unknown",
    }
}

fn deployment_id_from_path(path: &str) -> Option<i64> {
    let mut segments = path.trim_matches('/').split('/');
    while let Some(segment) = segments.next() {
        if segment == "deployments" {
            return segments.next()?.parse::<i64>().ok();
        }
    }
    None
}

pub fn apply_http_trace_layer<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &Request<Body>| {
                let route = request
                    .extensions()
                    .get::<MatchedPath>()
                    .map(MatchedPath::as_str)
                    .unwrap_or(request.uri().path());
                let path = request.uri().path();
                let client_ip = request
                    .extensions()
                    .get::<ConnectInfo<SocketAddr>>()
                    .map(|connect_info| connect_info.0.ip().to_string())
                    .unwrap_or_default();
                let deployment_id = deployment_id_from_path(path);

                tracing::info_span!(
                    "http_request",
                    http.method = %request.method(),
                    url.path = %path,
                    http.route = %route,
                    network.protocol.version = http_version(request.version()),
                    http.request_id = tracing::field::Empty,
                    http.response.status_code = tracing::field::Empty,
                    duration_ms = tracing::field::Empty,
                    deployment_id = deployment_id,
                    client_ip = %client_ip,
                )
            })
            .on_response(|response: &Response<_>, latency: Duration, span: &Span| {
                span.record("http.response.status_code", response.status().as_u16());
                span.record("duration_ms", latency.as_millis() as u64);
            })
            .on_failure(
                |error: ServerErrorsFailureClass, latency: Duration, span: &Span| {
                    span.record("duration_ms", latency.as_millis() as u64);
                    tracing::error!(parent: span, status = %error, "response failed");
                },
            ),
    )
}
