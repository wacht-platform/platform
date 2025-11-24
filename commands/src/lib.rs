use common::error::AppError;
use common::state::AppState;

pub trait Command {
    type Output;

    fn execute(
        self,
        app_state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self::Output, AppError>> + Send;
}

pub mod agent_execution_context;
pub mod agent_memory;
pub mod ai_agents;
pub mod ai_knowledge_base;
pub mod ai_tools;
pub mod ai_workflows;
pub mod api_key;
pub mod api_key_app;
pub mod billing;
pub mod conversation;
pub mod create_organization;
pub mod create_workspace;
mod delete_organization;
mod delete_workspace;
pub mod deployment;
pub mod deployment_email_template;
pub mod email;
pub mod embedding;
pub mod embedding_task;
pub mod notification;
mod organization_member;
mod organization_role;
pub mod process_document;
pub mod process_document_embedding;
pub mod project;
pub mod s3;
pub mod smtp;
mod update_organization;
mod update_workspace;
pub mod user;
pub mod user_identifiers;
pub mod webhook_app;
pub mod webhook_delivery;
pub mod webhook_endpoint;
pub mod webhook_storage;
pub mod webhook_subscription;
pub mod webhook_trigger;
pub mod workspace_member;
mod workspace_role;
pub use agent_execution_context::*;
pub use create_organization::*;
pub use create_workspace::*;
pub use delete_organization::*;
pub use delete_workspace::*;
pub use deployment::*;
pub use deployment_email_template::*;
pub use email::*;
pub use organization_member::*;
pub use organization_role::*;
pub use project::*;
pub use s3::*;
pub use update_organization::*;
pub use update_workspace::*;
pub use user::*;
pub use user_identifiers::*;
pub use workspace_member::*;
pub use workspace_role::*;

pub use agent_memory::*;
pub use ai_agents::*;
pub use ai_knowledge_base::*;
pub use ai_tools::*;
pub use ai_workflows::*;
pub use conversation::*;
pub use embedding::*;
pub use embedding_task::*;
pub use process_document::*;
pub use process_document_embedding::*;
pub use webhook_app::*;
pub use webhook_delivery::*;
pub use webhook_endpoint::*;
pub use webhook_storage::*;
pub use webhook_subscription::*;
pub use webhook_trigger::*;
pub use smtp::*;
