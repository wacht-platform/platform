use common::error::AppError;
use models::OrganizationInvitation;
use sqlx::{Postgres, QueryBuilder, Row};

pub struct GetOrganizationInvitationsQuery {
    deployment_id: i64,
    organization_id: i64,
    workspace_id: Option<i64>,
    include_deleted: bool,
}

impl GetOrganizationInvitationsQuery {
    pub fn new(deployment_id: i64, organization_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
            workspace_id: None,
            include_deleted: false,
        }
    }

    pub fn workspace_id(mut self, workspace_id: Option<i64>) -> Self {
        self.workspace_id = workspace_id;
        self
    }

    pub fn include_deleted(mut self, include_deleted: bool) -> Self {
        self.include_deleted = include_deleted;
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<OrganizationInvitation>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT
                oi.id, oi.created_at, oi.updated_at,
                oi.organization_id, oi.email,
                oi.initial_organization_role_id, oi.inviter_id, oi.workspace_id,
                oi.initial_workspace_role_id,
                COALESCE(oi.expired, false) AS expired,
                oi.expiry, oi.token,
                orole.name AS org_role_name,
                w.name AS workspace_name,
                wrole.name AS workspace_role_name
            FROM organization_invitations oi
            JOIN organizations o
              ON o.id = oi.organization_id
             AND o.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);
        qb.push(
            r#"
            LEFT JOIN organization_roles orole
              ON orole.id = oi.initial_organization_role_id
            LEFT JOIN workspaces w
              ON w.id = oi.workspace_id
            LEFT JOIN workspace_roles wrole
              ON wrole.id = oi.initial_workspace_role_id
            WHERE oi.organization_id =
            "#,
        );
        qb.push_bind(self.organization_id);
        if !self.include_deleted {
            qb.push(" AND oi.deleted_at IS NULL");
        }
        if let Some(ws_id) = self.workspace_id {
            qb.push(" AND oi.workspace_id = ");
            qb.push_bind(ws_id);
        }
        qb.push(" ORDER BY oi.created_at DESC");

        let rows = qb.build().fetch_all(executor).await?;

        let invitations = rows
            .into_iter()
            .map(|row| OrganizationInvitation {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                organization_id: row.get("organization_id"),
                email: row.get::<Option<String>, _>("email").unwrap_or_default(),
                initial_organization_role_id: row.get("initial_organization_role_id"),
                initial_organization_role_name: row.get("org_role_name"),
                inviter_id: row.get("inviter_id"),
                workspace_id: row.get("workspace_id"),
                workspace_name: row.get("workspace_name"),
                initial_workspace_role_id: row.get("initial_workspace_role_id"),
                initial_workspace_role_name: row.get("workspace_role_name"),
                expired: row.get("expired"),
                expiry: row.get("expiry"),
                token: row.get("token"),
            })
            .collect();

        Ok(invitations)
    }
}
