mod error;
pub mod response;
mod router;

pub use common::error::AppError;
pub use common::state::AppState as HttpState;

pub async fn console_router(app_state: HttpState) -> axum::Router {
    router::create_console_router(app_state).await
}

pub async fn backend_router(app_state: HttpState) -> axum::Router {
    router::create_backend_router(app_state).await
}

pub async fn frontend_router(app_state: HttpState) -> axum::Router {
    router::create_frontend_router(app_state).await
}
