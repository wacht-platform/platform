use common::error::AppError;
use common::state::AppState;

pub mod prelude {
    pub use super::Query;
    pub use common::error::AppError;
    pub use common::state::AppState;
    pub use std::result::Result as StdResult;
}

pub trait Query {
    type Output;

    fn execute(
        &self,
        app_state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self::Output, AppError>> + Send;
}

pub mod b2b;
pub mod deployment;
pub mod enterprise_sso;
pub mod invitation;
pub mod oauth;
pub mod oauth_runtime;
pub mod organization;
pub mod project;
pub mod segments;
pub mod signin;
pub mod storage;
pub mod user;
pub mod workspace;

pub mod agent_execution_context;
pub mod agent_integration;
pub mod ai_agent;
pub mod ai_knowledge_base;
pub mod ai_tool;
pub mod billing;
pub mod hybrid_search;
pub mod mcp_server;
pub mod plan_access;
pub use agent_execution_context::*;
pub use agent_integration::*;
pub use ai_agent::*;
pub use ai_knowledge_base::*;
pub use b2b::*;
pub use billing::*;
pub use deployment::*;
pub use enterprise_sso::*;
pub use invitation::*;
pub use oauth::*;
pub use oauth_runtime::*;
pub use organization::*;
pub use project::*;
pub use signin::*;
pub use storage::*;
pub use user::*;
pub use workspace::*;
pub mod agent_memory;
pub mod api_key;
pub mod api_key_audit;
pub mod api_key_gateway;
pub mod rate_limit_scheme;
pub mod sms;
pub mod webhook;
pub mod webhook_analytics;
pub use agent_memory::*;
pub use ai_tool::*;
pub use api_key_audit::*;
pub use hybrid_search::*;
pub use mcp_server::*;
pub use rate_limit_scheme::*;
pub use sms::*;
pub use webhook::*;
pub use webhook_analytics::*;
pub mod agent_session;
pub use agent_session::*;
pub mod ai_settings;
pub use ai_settings::*;
