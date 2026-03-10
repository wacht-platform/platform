use chrono::{DateTime, Utc};
use common::error::AppError;
use common::json_utils::{json_default, json_option_default, json_vec_default};
use models::api_key::ApiKey;
use queries::api_key::{
    GetOrganizationMembershipIdByUserAndOrganizationQuery,
    GetWorkspaceMembershipIdByUserAndWorkspaceQuery,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn build_api_key_model(
    id: i64,
    deployment_id: i64,
    app_slug: String,
    name: String,
    key_prefix: String,
    key_suffix: String,
    key_hash: String,
    permissions: Option<serde_json::Value>,
    metadata: Option<serde_json::Value>,
    rate_limit_scheme_slug: Option<String>,
    owner_user_id: Option<i64>,
    organization_id: Option<i64>,
    workspace_id: Option<i64>,
    organization_membership_id: Option<i64>,
    workspace_membership_id: Option<i64>,
    org_role_permissions: serde_json::Value,
    workspace_role_permissions: serde_json::Value,
    expires_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    is_active: Option<bool>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_reason: Option<String>,
) -> ApiKey {
    ApiKey {
        id,
        deployment_id,
        app_slug,
        name,
        key_prefix,
        key_suffix,
        key_hash,
        permissions: json_default(json_option_default(permissions, serde_json::json!([]))),
        metadata: json_option_default(metadata, serde_json::json!({})),
        rate_limits: vec![],
        rate_limit_scheme_slug,
        owner_user_id,
        organization_id,
        workspace_id,
        organization_membership_id,
        workspace_membership_id,
        org_role_permissions: json_vec_default(org_role_permissions),
        workspace_role_permissions: json_vec_default(workspace_role_permissions),
        expires_at,
        last_used_at,
        is_active: is_active.unwrap_or(true),
        created_at: created_at.unwrap_or_else(chrono::Utc::now),
        updated_at: updated_at.unwrap_or_else(chrono::Utc::now),
        revoked_at,
        revoked_reason,
    }
}

pub(super) fn user_not_member_error() -> AppError {
    AppError::BadRequest("user is not a member of the org".to_string())
}

pub(super) async fn resolve_org_membership_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Option<i64>,
    organization_id: Option<i64>,
) -> Result<Option<i64>, AppError> {
    if let (Some(user_id), Some(organization_id)) = (user_id, organization_id) {
        let membership_id =
            GetOrganizationMembershipIdByUserAndOrganizationQuery::new(user_id, organization_id)
                .execute_with_db(&mut **tx)
                .await?;
        if membership_id.is_none() {
            return Err(user_not_member_error());
        }
        Ok(membership_id)
    } else {
        Ok(None)
    }
}

pub(super) async fn resolve_workspace_membership_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Option<i64>,
    workspace_id: Option<i64>,
) -> Result<Option<i64>, AppError> {
    if let (Some(user_id), Some(workspace_id)) = (user_id, workspace_id) {
        let membership_id = GetWorkspaceMembershipIdByUserAndWorkspaceQuery::new(user_id, workspace_id)
            .execute_with_db(&mut **tx)
            .await?;
        if membership_id.is_none() {
            return Err(user_not_member_error());
        }
        Ok(membership_id)
    } else {
        Ok(None)
    }
}
