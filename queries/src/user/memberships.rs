use super::*;
use models::{
    Organization, OrganizationRole, UserOrganizationMembership, UserWorkspaceMembership, Workspace,
    WorkspaceRole,
};
use serde::de::DeserializeOwned;
use sqlx::{Postgres, QueryBuilder, Row};

fn parse_json_row_field<T: DeserializeOwned>(
    row: &sqlx::postgres::PgRow,
    field: &str,
    context: &str,
) -> Result<T, AppError> {
    serde_json::from_value(row.get(field))
        .map_err(|e| AppError::Internal(format!("Failed to parse {}: {}", context, e)))
}

pub struct GetUserOrganizationMembershipsQuery {
    deployment_id: i64,
    user_id: i64,
}

impl GetUserOrganizationMembershipsQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<UserOrganizationMembership>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT
                om.id,
                om.created_at,
                om.updated_at,
                om.organization_id,
                om.user_id,
                om.public_metadata,
                o.created_at AS org_created_at,
                o.updated_at AS org_updated_at,
                o.name AS org_name,
                o.image_url AS org_image_url,
                o.description AS org_description,
                o.member_count AS org_member_count,
                o.public_metadata AS org_public_metadata,
                o.private_metadata AS org_private_metadata,
                COALESCE(
                    (SELECT jsonb_agg(
                        jsonb_build_object(
                            'id', orole.id::text,
                            'created_at', orole.created_at,
                            'updated_at', orole.updated_at,
                            'name', orole.name,
                            'permissions', orole.permissions,
                            'is_deployment_level', CASE WHEN orole.organization_id IS NULL THEN true ELSE false END
                        ) ORDER BY orole.name
                    )
                    FROM organization_membership_roles omr
                    JOIN organization_roles orole ON omr.organization_role_id = orole.id
                    WHERE omr.organization_membership_id = om.id
                    ),
                    '[]'::jsonb
                ) AS roles
            FROM organization_memberships om
            JOIN organizations o
              ON om.organization_id = o.id
             AND o.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);
        qb.push(
            r#"
             AND o.deleted_at IS NULL
            JOIN users u
              ON om.user_id = u.id
             AND u.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);
        qb.push(
            r#"
            WHERE om.deleted_at IS NULL
              AND om.user_id =
            "#,
        );
        qb.push_bind(self.user_id);
        qb.push(" ORDER BY om.created_at DESC");

        let rows = qb.build().fetch_all(executor).await?;

        let memberships: Vec<UserOrganizationMembership> = rows
            .into_iter()
            .map(|row| -> Result<UserOrganizationMembership, AppError> {
                let roles: Vec<OrganizationRole> =
                    parse_json_row_field(&row, "roles", "user organization membership roles")?;

                let organization = Organization {
                    id: row.get("organization_id"),
                    created_at: row.get("org_created_at"),
                    updated_at: row.get("org_updated_at"),
                    name: row.get("org_name"),
                    image_url: row.get("org_image_url"),
                    description: row.get("org_description"),
                    member_count: row.get("org_member_count"),
                    public_metadata: row.get("org_public_metadata"),
                    private_metadata: row.get("org_private_metadata"),
                };

                Ok(UserOrganizationMembership {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    organization_id: row.get("organization_id"),
                    user_id: row.get("user_id"),
                    public_metadata: row.get("public_metadata"),
                    roles,
                    organization,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(memberships)
    }
}

pub struct GetUserWorkspaceMembershipsQuery {
    deployment_id: i64,
    user_id: i64,
}

impl GetUserWorkspaceMembershipsQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<UserWorkspaceMembership>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT
                wm.id,
                wm.created_at,
                wm.updated_at,
                wm.workspace_id,
                wm.organization_id,
                wm.organization_membership_id,
                wm.user_id,
                wm.public_metadata,
                w.created_at AS ws_created_at,
                w.updated_at AS ws_updated_at,
                w.name AS ws_name,
                w.image_url AS ws_image_url,
                w.description AS ws_description,
                w.member_count AS ws_member_count,
                w.public_metadata AS ws_public_metadata,
                w.private_metadata AS ws_private_metadata,
                COALESCE(
                    (SELECT jsonb_agg(
                        jsonb_build_object(
                            'id', wrole.id::text,
                            'created_at', wrole.created_at,
                            'updated_at', wrole.updated_at,
                            'name', wrole.name,
                            'permissions', wrole.permissions,
                            'is_deployment_level', CASE WHEN wrole.workspace_id IS NULL THEN true ELSE false END
                        ) ORDER BY wrole.name
                    )
                    FROM workspace_membership_roles wmr
                    JOIN workspace_roles wrole ON wmr.workspace_role_id = wrole.id
                    WHERE wmr.workspace_membership_id = wm.id
                    ),
                    '[]'::jsonb
                ) AS roles
            FROM workspace_memberships wm
            JOIN workspaces w
              ON wm.workspace_id = w.id
             AND w.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);
        qb.push(
            r#"
             AND w.deleted_at IS NULL
            JOIN users u
              ON wm.user_id = u.id
             AND u.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);
        qb.push(
            r#"
            WHERE wm.deleted_at IS NULL
              AND wm.user_id =
            "#,
        );
        qb.push_bind(self.user_id);
        qb.push(" ORDER BY wm.created_at DESC");

        let rows = qb.build().fetch_all(executor).await?;

        let memberships: Vec<UserWorkspaceMembership> = rows
            .into_iter()
            .map(|row| -> Result<UserWorkspaceMembership, AppError> {
                let roles: Vec<WorkspaceRole> =
                    parse_json_row_field(&row, "roles", "user workspace membership roles")?;

                let workspace = Workspace {
                    id: row.get("workspace_id"),
                    created_at: row.get("ws_created_at"),
                    updated_at: row.get("ws_updated_at"),
                    name: row.get("ws_name"),
                    image_url: row.get("ws_image_url"),
                    description: row.get("ws_description"),
                    member_count: row.get("ws_member_count"),
                    public_metadata: row.get("ws_public_metadata"),
                    private_metadata: row.get("ws_private_metadata"),
                };

                Ok(UserWorkspaceMembership {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    workspace_id: row.get("workspace_id"),
                    organization_id: row.get("organization_id"),
                    organization_membership_id: row.get("organization_membership_id"),
                    user_id: row.get("user_id"),
                    public_metadata: row.get("public_metadata"),
                    roles,
                    workspace,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(memberships)
    }
}
