pub mod agent_integrations;
pub mod ai_agents;
pub mod ai_execution_context;
pub mod ai_knowledge_base;
pub mod ai_settings;
pub mod ai_tools;
pub mod analytics;
pub mod api_key_app;
pub mod api_key_audit;
pub mod api_key_key;
pub mod api_key_rate_limit;
pub mod api_key_shared;
pub mod b2b_entity;
pub mod b2b_membership;
pub mod b2b_query;
pub mod billing;
pub mod billing_webhook;
pub mod connection;
pub mod enterprise_sso;
mod error;
pub mod mcp_servers;
pub mod notifications;
pub mod oauth_app;
pub mod oauth_client;
pub mod oauth_grant;
pub mod oauth_runtime;
pub mod oauth_scope;
pub mod oauth_shared;
pub mod project;
pub mod response;
mod router;
pub mod segments;
pub mod session_tickets;
pub mod settings;
pub mod upload;
pub mod user_core;
pub mod user_identifier;
pub mod user_invitation;
pub mod webhook_analytics;
pub mod webhook_apps;
pub mod webhook_deliveries;
pub mod webhook_dispatch;
pub mod webhook_endpoints;
pub mod webhook_replay;

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
