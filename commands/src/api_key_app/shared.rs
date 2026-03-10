use common::error::AppError;
use models::api_key::ApiAuthApp;
use queries::api_key::{
    GetOrganizationMembershipIdByUserAndOrganizationQuery,
    GetWorkspaceMembershipIdByUserAndWorkspaceQuery,
};

fn parse_string_vec(value: serde_json::Value) -> Vec<String> {
    serde_json::from_value(value).unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_api_auth_app_model(
    deployment_id: i64,
    user_id: Option<i64>,
    organization_id: Option<i64>,
    workspace_id: Option<i64>,
    app_slug: String,
    name: String,
    description: Option<String>,
    is_active: Option<bool>,
    key_prefix: String,
    permissions: serde_json::Value,
    resources: serde_json::Value,
    rate_limit_scheme_slug: Option<String>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
) -> ApiAuthApp {
    ApiAuthApp {
        deployment_id,
        user_id,
        organization_id,
        workspace_id,
        app_slug,
        name,
        description,
        is_active: is_active.unwrap_or(true),
        key_prefix,
        permissions: parse_string_vec(permissions),
        resources: parse_string_vec(resources),
        rate_limits: vec![],
        rate_limit_scheme_slug,
        created_at: created_at.unwrap_or_else(chrono::Utc::now),
        updated_at: updated_at.unwrap_or_else(chrono::Utc::now),
        deleted_at,
    }
}

pub(super) async fn ensure_user_exists(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    user_id: i64,
) -> Result<(), AppError> {
    let user = sqlx::query!(
        r#"
        SELECT id
        FROM users
        WHERE id = $1
          AND deployment_id = $2
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        user_id,
        deployment_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    if user.is_none() {
        return Err(AppError::Validation(
            "user_id does not exist for this deployment".to_string(),
        ));
    }

    Ok(())
}

async fn ensure_organization_exists(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    organization_id: i64,
) -> Result<(), AppError> {
    let organization = sqlx::query!(
        r#"
        SELECT id
        FROM organizations
        WHERE id = $1
          AND deployment_id = $2
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        organization_id,
        deployment_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    if organization.is_none() {
        return Err(AppError::Validation(
            "organization_id does not exist for this deployment".to_string(),
        ));
    }

    Ok(())
}

async fn resolve_workspace_organization(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    workspace_id: i64,
) -> Result<i64, AppError> {
    let workspace = sqlx::query!(
        r#"
        SELECT organization_id
        FROM workspaces
        WHERE id = $1
          AND deployment_id = $2
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        workspace_id,
        deployment_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    workspace.map(|w| w.organization_id).ok_or_else(|| {
        AppError::Validation("workspace_id does not exist for this deployment".to_string())
    })
}

pub(super) async fn resolve_scope_organization(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    organization_id: Option<i64>,
    workspace_id: Option<i64>,
) -> Result<Option<i64>, AppError> {
    let mut resolved_organization_id = organization_id;

    if let Some(workspace_id) = workspace_id {
        let workspace_org_id =
            resolve_workspace_organization(conn, deployment_id, workspace_id).await?;
        if let Some(explicit_org_id) = resolved_organization_id
            && explicit_org_id != workspace_org_id
        {
            return Err(AppError::Validation(
                "workspace_id does not belong to organization_id".to_string(),
            ));
        }
        resolved_organization_id = Some(workspace_org_id);
    }

    if let Some(org_id) = resolved_organization_id {
        ensure_organization_exists(conn, deployment_id, org_id).await?;
    }

    Ok(resolved_organization_id)
}

pub(super) async fn ensure_user_in_organization(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    organization_id: i64,
) -> Result<(), AppError> {
    let membership_id =
        GetOrganizationMembershipIdByUserAndOrganizationQuery::new(user_id, organization_id)
            .execute_with_db(conn)
            .await?;

    if membership_id.is_none() {
        return Err(AppError::Validation(
            "user_id is not a member of organization_id".to_string(),
        ));
    }

    Ok(())
}

pub(super) async fn ensure_user_in_workspace(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    workspace_id: i64,
) -> Result<(), AppError> {
    let membership_id = GetWorkspaceMembershipIdByUserAndWorkspaceQuery::new(user_id, workspace_id)
        .execute_with_db(conn)
        .await?;

    if membership_id.is_none() {
        return Err(AppError::Validation(
            "user_id is not a member of workspace_id".to_string(),
        ));
    }

    Ok(())
}
