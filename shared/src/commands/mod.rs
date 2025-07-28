use crate::{error::AppError, state::AppState};

pub trait Command {
    type Output;

    fn execute(
        self,
        app_state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self::Output, AppError>> + Send;
}

pub mod agent_execution_context;
pub mod ai_agents;
pub mod ai_knowledge_base;
pub mod ai_tools;
pub mod ai_workflows;
pub mod create_organization;
pub mod create_workspace;
mod delete_organization;
mod delete_workspace;
pub mod deployment;
pub mod deployment_billing_plan;
pub mod deployment_email_template;
pub mod deployment_stripe_account;
pub mod deployment_subscription;
pub mod email;
pub mod embedding;
pub mod memory_boundaries;
pub mod memory_consolidation;
pub mod agent_memory;
mod organization_member;
mod organization_role;
pub mod project;
pub mod s3;
mod update_organization;
mod update_workspace;
pub mod user;
pub mod user_identifiers;
mod workspace_role;
pub use agent_execution_context::*;
pub use create_organization::*;
pub use create_workspace::*;
pub use delete_organization::*;
pub use delete_workspace::*;
pub use deployment::*;
pub use deployment_billing_plan::*;
pub use deployment_email_template::*;
pub use deployment_stripe_account::*;
pub use deployment_subscription::*;
pub use email::*;
pub use organization_member::*;
pub use organization_role::*;
pub use project::*;
pub use s3::*;
pub use update_organization::*;
pub use update_workspace::*;
pub use user::*;
pub use user_identifiers::*;
pub use workspace_role::*;

pub use ai_agents::*;
pub use ai_knowledge_base::*;
pub use ai_tools::*;
pub use ai_workflows::*;
pub use embedding::*;
pub use memory_boundaries::*;
pub use memory_consolidation::*;
pub use agent_memory::*;
