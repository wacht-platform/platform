use axum::{extract::Request, middleware::Next, response::Response};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformSource {
    Console,
    Backend,
}

pub async fn mark_console_platform_source(mut req: Request, next: Next) -> Response {
    req.extensions_mut().insert(PlatformSource::Console);
    next.run(req).await
}

pub async fn mark_backend_platform_source(mut req: Request, next: Next) -> Response {
    req.extensions_mut().insert(PlatformSource::Backend);
    next.run(req).await
}
