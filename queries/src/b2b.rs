use sqlx::{Postgres, QueryBuilder, Row, query_as};

use crate::prelude::*;
use models::{
    DeploymentOrganizationRole, DeploymentWorkspaceRole, Organization, OrganizationDetails,
    OrganizationMemberDetails, OrganizationRole, Workspace, WorkspaceDetails,
    WorkspaceMemberDetails, WorkspaceRole, WorkspaceWithOrganizationName,
};

pub struct GetDeploymentWorkspaceRolesQuery {
    deployment_id: i64,
}

impl GetDeploymentWorkspaceRolesQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentWorkspaceRolesQuery {
    type Output = Vec<DeploymentWorkspaceRole>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rows = query_as!(
            DeploymentWorkspaceRole,
            r#"
            SELECT * FROM workspace_roles WHERE deployment_id = $1"#,
            self.deployment_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(rows)
    }
}

pub struct GetDeploymentOrganizationRolesQuery {
    deployment_id: i64,
}

impl GetDeploymentOrganizationRolesQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentOrganizationRolesQuery {
    type Output = Vec<DeploymentOrganizationRole>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rows = query_as!(
            DeploymentOrganizationRole,
            r#"SELECT * FROM organization_roles WHERE deployment_id = $1"#,
            self.deployment_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(rows)
    }
}

pub struct DeploymentOrganizationListQuery {
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
    deployment_id: i64,
    search: Option<String>,
}

impl DeploymentOrganizationListQuery {
    pub fn new(id: i64) -> Self {
        Self {
            offset: 0,
            sort_key: None,
            sort_order: None,
            limit: 10,
            deployment_id: id,
            search: None,
        }
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = offset;
        self
    }

    pub fn limit(mut self, limit: i32) -> Self {
        self.limit = limit;
        self
    }

    pub fn sort_key(mut self, sort_key: Option<String>) -> Self {
        self.sort_key = sort_key;
        self
    }

    pub fn sort_order(mut self, sort_order: Option<String>) -> Self {
        self.sort_order = sort_order;
        self
    }

    pub fn search(mut self, search: Option<String>) -> Self {
        self.search = search;
        self
    }
}

impl Query for DeploymentOrganizationListQuery {
    type Output = Vec<Organization>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT
                o.id, o.created_at, o.updated_at,
                o.name, o.image_url, o.description, o.member_count,
                o.public_metadata, o.private_metadata
            FROM organizations o
            WHERE o.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);

        if let Some(search) = &self.search {
            if !search.trim().is_empty() {
                let pattern = format!("%{}%", search.trim());
                qb.push(" AND o.name ILIKE ");
                qb.push_bind(pattern);
            }
        }

        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");

        // Sanitize sort key to prevent SQL injection (though unlikely with current usage)
        let valid_sort_keys = ["created_at", "name", "member_count", "updated_at"];
        let safe_sort_key = if valid_sort_keys.contains(&sort_key) {
            sort_key
        } else {
            "created_at"
        };

        let safe_sort_order = if sort_order.to_lowercase() == "asc" {
            "ASC"
        } else {
            "DESC"
        };

        qb.push(format!(" ORDER BY o.{} {}", safe_sort_key, safe_sort_order));

        qb.push(" OFFSET ");
        qb.push_bind(self.offset);
        qb.push(" LIMIT ");
        qb.push_bind(self.limit);

        let rows = qb.build().fetch_all(&app_state.db_pool).await?;

        Ok(rows
            .into_iter()
            .map(|row| Organization {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                name: row.get("name"),
                image_url: row.get("image_url"),
                description: row.get("description"),
                member_count: row.get("member_count"),
                public_metadata: row.get("public_metadata"),
                private_metadata: row.get("private_metadata"),
            })
            .collect())
    }
}

pub struct DeploymentWorkspaceListQuery {
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
    deployment_id: i64,
    search: Option<String>,
}

impl DeploymentWorkspaceListQuery {
    pub fn new(id: i64) -> Self {
        Self {
            offset: 0,
            sort_key: None,
            sort_order: None,
            limit: 10,
            deployment_id: id,
            search: None,
        }
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = offset;
        self
    }

    pub fn limit(mut self, limit: i32) -> Self {
        self.limit = limit;
        self
    }

    pub fn sort_key(mut self, sort_key: Option<String>) -> Self {
        self.sort_key = sort_key;
        self
    }

    pub fn sort_order(mut self, sort_order: Option<String>) -> Self {
        self.sort_order = sort_order;
        self
    }

    pub fn search(mut self, search: Option<String>) -> Self {
        self.search = search;
        self
    }
}

impl Query for DeploymentWorkspaceListQuery {
    type Output = Vec<WorkspaceWithOrganizationName>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT
                w.id, w.created_at, w.updated_at, w.deleted_at,
                w.name, w.image_url, w.description, w.member_count,
                w.public_metadata, w.private_metadata,
                o.name AS organization_name
            FROM workspaces w
            LEFT JOIN organizations o ON w.organization_id = o.id
            WHERE w.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);

        if let Some(search) = &self.search {
            if !search.trim().is_empty() {
                let pattern = format!("%{}%", search.trim());
                qb.push(" AND (w.name ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR o.name ILIKE ");
                qb.push_bind(pattern);
                qb.push(")");
            }
        }

        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");

        // Sanitize sort key
        let valid_sort_keys = [
            "created_at",
            "name",
            "member_count",
            "updated_at",
            "organization_name",
        ];
        let safe_sort_key = if valid_sort_keys.contains(&sort_key) {
            sort_key
        } else {
            "created_at"
        };

        let safe_sort_order = if sort_order.to_lowercase() == "asc" {
            "ASC"
        } else {
            "DESC"
        };

        // Handle sorting by organization name which is on joined table 'o'
        if safe_sort_key == "organization_name" {
            qb.push(format!(" ORDER BY o.name {}", safe_sort_order));
        } else {
            qb.push(format!(" ORDER BY w.{} {}", safe_sort_key, safe_sort_order));
        }

        qb.push(" OFFSET ");
        qb.push_bind(self.offset);
        qb.push(" LIMIT ");
        qb.push_bind(self.limit);

        let rows = qb.build().fetch_all(&app_state.db_pool).await?;

        Ok(rows
            .into_iter()
            .map(|row| WorkspaceWithOrganizationName {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                name: row.get("name"),
                image_url: row.get("image_url"),
                description: row.get("description"),
                member_count: row.get("member_count"),
                organization_name: row.get("organization_name"),
            })
            .collect())
    }
}

pub struct GetOrganizationDetailsQuery {
    deployment_id: i64,
    organization_id: i64,
}

impl GetOrganizationDetailsQuery {
    pub fn new(deployment_id: i64, organization_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
        }
    }
}

impl Query for GetOrganizationDetailsQuery {
    type Output = OrganizationDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get organization basic info
        let org_row = sqlx::query!(
            r#"
            SELECT
                o.id, o.created_at, o.updated_at,
                o.name, o.image_url, o.description, o.member_count,
                o.public_metadata, o.private_metadata
            FROM organizations o
            WHERE o.deployment_id = $1 AND o.id = $2
            "#,
            self.deployment_id,
            self.organization_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        // Get organization roles with permissions (both deployment-level and organization-specific)
        let role_rows = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, name, permissions, organization_id
            FROM organization_roles
            WHERE (deployment_id = $1 AND organization_id IS NULL)
               OR organization_id = $2
            "#,
            self.deployment_id,
            self.organization_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        let roles: Vec<OrganizationRole> = role_rows
            .into_iter()
            .map(|row| OrganizationRole {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                name: row.name,
                permissions: row.permissions,
                is_deployment_level: row.organization_id.is_none(),
            })
            .collect();

        // Get organization workspaces
        let workspace_rows = sqlx::query!(
            r#"
            SELECT
                id, created_at, updated_at,
                name, image_url as "image_url?", description as "description?", member_count,
                public_metadata, private_metadata
            FROM workspaces
            WHERE organization_id = $1
            ORDER BY created_at DESC
            "#,
            self.organization_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        let workspaces: Vec<Workspace> = workspace_rows
            .into_iter()
            .map(|row| Workspace {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                name: row.name,
                image_url: row.image_url.unwrap_or_default(),
                description: row.description.unwrap_or_default(),
                member_count: row.member_count,
                public_metadata: row.public_metadata,
                private_metadata: row.private_metadata,
            })
            .collect();

        let segments = sqlx::query_as!(
            models::Segment,
            r#"
            SELECT s.id, s.created_at, s.updated_at, s.deleted_at, s.deployment_id, s.name,
                   s.type as "segment_type: _"
            FROM segments s
            JOIN organization_segments os ON s.id = os.segment_id
            WHERE os.organization_id = $1
            "#,
            self.organization_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(OrganizationDetails {
            id: org_row.id,
            created_at: org_row.created_at,
            updated_at: org_row.updated_at,
            name: org_row.name,
            image_url: org_row.image_url,
            description: org_row.description.unwrap_or_default(),
            member_count: org_row.member_count,
            public_metadata: org_row.public_metadata,
            private_metadata: org_row.private_metadata,
            roles,
            workspaces,
            segments,
        })
    }
}

pub struct GetWorkspaceDetailsQuery {
    deployment_id: i64,
    workspace_id: i64,
}

impl GetWorkspaceDetailsQuery {
    pub fn new(deployment_id: i64, workspace_id: i64) -> Self {
        Self {
            deployment_id,
            workspace_id,
        }
    }
}

impl Query for GetWorkspaceDetailsQuery {
    type Output = WorkspaceDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get workspace basic info with organization name
        let workspace_row = sqlx::query!(
            r#"
            SELECT
                w.id, w.created_at, w.updated_at,
                w.name, w.image_url, w.description, w.member_count,
                w.public_metadata, w.private_metadata, w.organization_id,
                o.name as "organization_name?"
            FROM workspaces w
            LEFT JOIN organizations o ON w.organization_id = o.id
            WHERE w.deployment_id = $1 AND w.id = $2
            "#,
            self.deployment_id,
            self.workspace_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        // Get workspace roles with permissions (both deployment-level and workspace-specific)
        let role_rows = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, name, permissions, workspace_id
            FROM workspace_roles
            WHERE (deployment_id = $1 AND workspace_id IS NULL)
               OR workspace_id = $2
            "#,
            self.deployment_id,
            self.workspace_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        let roles: Vec<WorkspaceRole> = role_rows
            .into_iter()
            .map(|row| WorkspaceRole {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                name: row.name,
                permissions: row.permissions,
                is_deployment_level: row.workspace_id.is_none(),
            })
            .collect();

        let segments = sqlx::query_as!(
            models::Segment,
            r#"
            SELECT s.id, s.created_at, s.updated_at, s.deleted_at, s.deployment_id, s.name,
                   s.type as "segment_type: _"
            FROM segments s
            JOIN workspace_segments ws ON s.id = ws.segment_id
            WHERE ws.workspace_id = $1
            "#,
            self.workspace_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(WorkspaceDetails {
            id: workspace_row.id,
            created_at: workspace_row.created_at,
            updated_at: workspace_row.updated_at,
            name: workspace_row.name,
            image_url: workspace_row.image_url,
            description: workspace_row.description,
            member_count: workspace_row.member_count as i32,
            public_metadata: workspace_row.public_metadata,
            private_metadata: workspace_row.private_metadata,
            organization_id: workspace_row.organization_id,
            organization_name: workspace_row.organization_name.unwrap_or_default(),
            roles,
            segments,
        })
    }
}

pub struct GetOrganizationMembersQuery {
    organization_id: i64,
    offset: i64,
    limit: i32,
    search: Option<String>,
    sort_key: Option<String>,
    sort_order: Option<String>,
}

impl GetOrganizationMembersQuery {
    pub fn new(organization_id: i64) -> Self {
        Self {
            organization_id,
            offset: 0,
            limit: 20,
            search: None,
            sort_key: None,
            sort_order: None,
        }
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = offset;
        self
    }

    pub fn limit(mut self, limit: i32) -> Self {
        self.limit = limit;
        self
    }

    pub fn search(mut self, search: Option<String>) -> Self {
        self.search = search;
        self
    }

    pub fn sort_key(mut self, sort_key: Option<String>) -> Self {
        self.sort_key = sort_key;
        self
    }

    pub fn sort_order(mut self, sort_order: Option<String>) -> Self {
        self.sort_order = sort_order;
        self
    }
}

impl Query for GetOrganizationMembersQuery {
    type Output = (Vec<OrganizationMemberDetails>, bool);

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT
                om.id, om.created_at, om.updated_at,
                om.organization_id, om.user_id,
                om.public_metadata,
                u.first_name, u.last_name, u.username,
                u.created_at as user_created_at,
                e.email_address as "primary_email_address",
                p.phone_number as "primary_phone_number",
                COALESCE(
                    jsonb_agg(
                        DISTINCT jsonb_build_object(
                            'id', orole.id::text,
                            'created_at', orole.created_at,
                            'updated_at', orole.updated_at,
                            'name', orole.name,
                            'permissions', orole.permissions,
                            'is_deployment_level', CASE WHEN orole.organization_id IS NULL THEN true ELSE false END
                        )
                    ) FILTER (WHERE orole.id IS NOT NULL),
                    '[]'::jsonb
                ) as "roles"
            FROM organization_memberships om
            JOIN users u ON om.user_id = u.id AND u.deleted_at IS NULL
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            LEFT JOIN organization_membership_roles omr ON omr.organization_membership_id = om.id
            LEFT JOIN organization_roles orole ON omr.organization_role_id = orole.id
            WHERE om.deleted_at IS NULL AND om.organization_id =
            "#,
        );

        qb.push_bind(self.organization_id);

        if let Some(search) = &self.search {
            if !search.trim().is_empty() {
                let pattern = format!("%{}%", search.trim());
                qb.push(" AND (u.first_name ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR u.last_name ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR u.username ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR e.email_address ILIKE ");
                qb.push_bind(pattern);
                qb.push(")");
            }
        }

        qb.push(" GROUP BY om.id, om.created_at, om.updated_at, om.organization_id, om.user_id, om.public_metadata, u.first_name, u.last_name, u.username, u.created_at, e.email_address, p.phone_number");

        let sort_column = match self.sort_key.as_deref() {
            Some("first_name") => "u.first_name",
            Some("last_name") => "u.last_name",
            Some("email") => "e.email_address",
            Some("username") => "u.username",
            Some("created_at") => "om.created_at",
            _ => "om.created_at",
        };

        let sort_direction = match self.sort_order.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC",
        };

        qb.push(format!(" ORDER BY {} {}", sort_column, sort_direction));

        qb.push(" LIMIT ");
        qb.push_bind((self.limit + 1) as i64);
        qb.push(" OFFSET ");
        qb.push_bind(self.offset);

        let member_rows = qb.build().fetch_all(&app_state.db_pool).await?;

        let has_more = member_rows.len() > self.limit as usize;
        let members: Vec<OrganizationMemberDetails> = member_rows
            .into_iter()
            .take(self.limit as usize)
            .map(|row| {
                let roles_json: serde_json::Value = row.get("roles");
                let roles_array = roles_json.as_array().unwrap();

                let roles: Vec<OrganizationRole> = roles_array
                    .iter()
                    .filter_map(|role_json| {
                        serde_json::from_value::<OrganizationRole>(role_json.clone()).ok()
                    })
                    .collect();

                OrganizationMemberDetails {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    organization_id: row.get("organization_id"),
                    user_id: row.get("user_id"),
                    roles,
                    public_metadata: row.get("public_metadata"),
                    first_name: row.get("first_name"),
                    last_name: row.get("last_name"),
                    username: row.get("username"),
                    primary_email_address: row.get("primary_email_address"),
                    primary_phone_number: row.get("primary_phone_number"),
                    user_created_at: row.get("user_created_at"),
                }
            })
            .collect();

        Ok((members, has_more))
    }
}

pub struct GetWorkspaceMembersQuery {
    workspace_id: i64,
    offset: i64,
    limit: i32,
    search: Option<String>,
    sort_key: Option<String>,
    sort_order: Option<String>,
}

impl GetWorkspaceMembersQuery {
    pub fn new(workspace_id: i64) -> Self {
        Self {
            workspace_id,
            offset: 0,
            limit: 20,
            search: None,
            sort_key: None,
            sort_order: None,
        }
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = offset;
        self
    }

    pub fn limit(mut self, limit: i32) -> Self {
        self.limit = limit;
        self
    }

    pub fn search(mut self, search: Option<String>) -> Self {
        self.search = search;
        self
    }

    pub fn sort_key(mut self, sort_key: Option<String>) -> Self {
        self.sort_key = sort_key;
        self
    }

    pub fn sort_order(mut self, sort_order: Option<String>) -> Self {
        self.sort_order = sort_order;
        self
    }
}

impl Query for GetWorkspaceMembersQuery {
    type Output = (Vec<WorkspaceMemberDetails>, bool);

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT
                wm.id, wm.created_at, wm.updated_at,
                wm.workspace_id, wm.user_id,
                wm.public_metadata,
                u.first_name, u.last_name, u.username,
                u.created_at as user_created_at,
                e.email_address as "primary_email_address",
                p.phone_number as "primary_phone_number",
                COALESCE(
                    jsonb_agg(
                        DISTINCT jsonb_build_object(
                            'id', wrole.id::text,
                            'created_at', wrole.created_at,
                            'updated_at', wrole.updated_at,
                            'name', wrole.name,
                            'permissions', wrole.permissions,
                            'is_deployment_level', CASE WHEN wrole.workspace_id IS NULL THEN true ELSE false END
                        )
                    ) FILTER (WHERE wrole.id IS NOT NULL),
                    '[]'::jsonb
                ) as "roles"
            FROM workspace_memberships wm
            JOIN users u ON wm.user_id = u.id AND u.deleted_at IS NULL
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            LEFT JOIN workspace_membership_roles wmr ON wmr.workspace_membership_id = wm.id
            LEFT JOIN workspace_roles wrole ON wmr.workspace_role_id = wrole.id
            WHERE wm.deleted_at IS NULL AND wm.workspace_id =
            "#,
        );

        qb.push_bind(self.workspace_id);

        if let Some(search) = &self.search {
            if !search.trim().is_empty() {
                let pattern = format!("%{}%", search.trim());
                qb.push(" AND (u.first_name ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR u.last_name ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR u.username ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR e.email_address ILIKE ");
                qb.push_bind(pattern);
                qb.push(")");
            }
        }

        qb.push(" GROUP BY wm.id, wm.created_at, wm.updated_at, wm.workspace_id, wm.user_id, wm.public_metadata, u.first_name, u.last_name, u.username, u.created_at, e.email_address, p.phone_number");

        // Sorting
        let sort_column = match self.sort_key.as_deref() {
            Some("first_name") => "u.first_name",
            Some("last_name") => "u.last_name",
            Some("email") => "e.email_address",
            Some("username") => "u.username",
            Some("created_at") => "wm.created_at",
            _ => "wm.created_at",
        };

        let sort_direction = match self.sort_order.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC",
        };

        qb.push(format!(" ORDER BY {} {}", sort_column, sort_direction));

        qb.push(" LIMIT ");
        qb.push_bind((self.limit + 1) as i64);
        qb.push(" OFFSET ");
        qb.push_bind(self.offset);

        let member_rows = qb.build().fetch_all(&app_state.db_pool).await?;

        let has_more = member_rows.len() > self.limit as usize;
        let members: Vec<WorkspaceMemberDetails> = member_rows
            .into_iter()
            .take(self.limit as usize)
            .map(|row| {
                let roles_json: serde_json::Value = row.get("roles");
                let roles_array = roles_json.as_array().unwrap();

                let roles: Vec<WorkspaceRole> = roles_array
                    .iter()
                    .filter_map(|role_json| {
                        serde_json::from_value::<WorkspaceRole>(role_json.clone()).ok()
                    })
                    .collect();

                WorkspaceMemberDetails {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    workspace_id: row.get("workspace_id"),
                    user_id: row.get("user_id"),
                    roles,
                    public_metadata: row.get("public_metadata"),
                    first_name: row.get("first_name"),
                    last_name: row.get("last_name"),
                    username: row.get("username"),
                    primary_email_address: row.get("primary_email_address"),
                    primary_phone_number: row.get("primary_phone_number"),
                    user_created_at: row.get("user_created_at"),
                }
            })
            .collect();

        Ok((members, has_more))
    }
}
