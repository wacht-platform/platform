mod error;
pub mod agent_integrations;
pub mod analytics;
pub mod ai_settings;
pub mod connection;
pub mod enterprise_sso;
pub mod notifications;
pub mod project;
pub mod response;
pub mod session_tickets;
pub mod segments;
mod router;

pub use common::error::AppError;
pub use common::state::AppState;

pub async fn console_router(app_state: AppState) -> axum::Router {
    router::create_console_router(app_state).await
}

pub async fn backend_router(app_state: AppState) -> axum::Router {
    router::create_backend_router(app_state).await
}

pub async fn frontend_router(app_state: AppState) -> axum::Router {
    router::create_frontend_router(app_state).await
}

pub async fn oauth_router(app_state: AppState) -> axum::Router {
    router::create_oauth_router(app_state).await
}
