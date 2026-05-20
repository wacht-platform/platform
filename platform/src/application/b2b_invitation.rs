use commands::{CreateOrganizationInvitationCommand, DiscardOrganizationInvitationCommand};
use common::db_router::ReadConsistency;
use common::error::AppError;
use models::OrganizationInvitation;
use queries::GetOrganizationInvitationsQuery;
use serde::Serialize;

use crate::application::AppState;

const DEFAULT_INVITATION_EXPIRY_DAYS: i64 = 10;

pub async fn list_organization_invitations(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    workspace_id: Option<i64>,
    include_deleted: bool,
) -> Result<Vec<OrganizationInvitation>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetOrganizationInvitationsQuery::new(deployment_id, organization_id)
        .workspace_id(workspace_id)
        .include_deleted(include_deleted)
        .execute_with_db(reader)
        .await
}

pub struct CreateOrganizationInvitationInput {
    pub email: String,
    pub initial_organization_role_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub initial_workspace_role_id: Option<i64>,
    pub expiry_days: Option<i64>,
}

/// Shape returned from create + resend. Admin tools build their own delivery
/// (email, Slack, in-app); we hand them the token + scope so they can render
/// the accept URL however they like.
#[derive(Serialize)]
pub struct OrganizationInvitationSummary {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    pub token: String,
    pub email: String,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub organization_id: i64,
    pub organization_name: String,
    #[serde(default, with = "models::utils::serde::i64_as_string_option")]
    pub workspace_id: Option<i64>,
}

pub async fn create_organization_invitation(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    input: CreateOrganizationInvitationInput,
) -> Result<OrganizationInvitationSummary, AppError> {
    let invitation_id = app_state.sf.next_id()? as i64;
    let created = CreateOrganizationInvitationCommand {
        deployment_id,
        organization_id,
        invitation_id,
        email: input.email,
        initial_organization_role_id: input.initial_organization_role_id,
        workspace_id: input.workspace_id,
        initial_workspace_role_id: input.initial_workspace_role_id,
        expiry_days: input.expiry_days.unwrap_or(DEFAULT_INVITATION_EXPIRY_DAYS),
    }
    .execute_with_pool(app_state.db_router.writer())
    .await?;

    Ok(OrganizationInvitationSummary {
        id: created.id,
        token: created.token,
        email: created.email,
        organization_id,
        workspace_id: created.workspace_id,
        organization_name: created.organization_name,
    })
}

pub async fn discard_organization_invitation(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    invitation_id: i64,
) -> Result<(), AppError> {
    DiscardOrganizationInvitationCommand {
        deployment_id,
        organization_id,
        invitation_id,
    }
    .execute_with_db(app_state.db_router.writer())
    .await
}
