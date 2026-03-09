use super::*;
pub(in crate::project) struct DeploymentB2bBootstrapInsert {
    settings_row_id: i64,
    workspace_creator_role_id: i64,
    workspace_member_role_id: i64,
    org_creator_role_id: i64,
    org_member_role_id: i64,
    b2b_settings: DeploymentB2bSettingsWithRoles,
}

#[derive(Default)]
pub(in crate::project) struct DeploymentB2bBootstrapInsertBuilder {
    settings_row_id: Option<i64>,
    workspace_creator_role_id: Option<i64>,
    workspace_member_role_id: Option<i64>,
    org_creator_role_id: Option<i64>,
    org_member_role_id: Option<i64>,
    b2b_settings: Option<DeploymentB2bSettingsWithRoles>,
}

impl DeploymentB2bBootstrapInsert {
    pub(in crate::project) fn builder() -> DeploymentB2bBootstrapInsertBuilder {
        DeploymentB2bBootstrapInsertBuilder::default()
    }

    pub(in crate::project) async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres> + Send,
    {
        let mut tx = acquirer.begin().await?;
        let now = chrono::Utc::now();
        let deployment_id = self.b2b_settings.settings.deployment_id;

        self.insert_workspace_role(
            tx.as_mut(),
            self.workspace_creator_role_id,
            deployment_id,
            &self.b2b_settings.default_workspace_creator_role,
            now,
        )
        .await?;
        self.insert_workspace_role(
            tx.as_mut(),
            self.workspace_member_role_id,
            deployment_id,
            &self.b2b_settings.default_workspace_member_role,
            now,
        )
        .await?;
        self.insert_organization_role(
            tx.as_mut(),
            self.org_creator_role_id,
            deployment_id,
            &self.b2b_settings.default_org_creator_role,
            now,
        )
        .await?;
        self.insert_organization_role(
            tx.as_mut(),
            self.org_member_role_id,
            deployment_id,
            &self.b2b_settings.default_org_member_role,
            now,
        )
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO deployment_b2b_settings (
                id,
                deployment_id,
                organizations_enabled,
                workspaces_enabled,
                ip_allowlist_per_org_enabled,
                max_allowed_org_members,
                max_allowed_workspace_members,
                allow_org_deletion,
                allow_workspace_deletion,
                custom_org_role_enabled,
                custom_workspace_role_enabled,
                default_workspace_creator_role_id,
                default_workspace_member_role_id,
                default_org_creator_role_id,
                default_org_member_role_id,
                limit_org_creation_per_user,
                limit_workspace_creation_per_org,
                org_creation_per_user_count,
                workspaces_per_org_count,
                allow_users_to_create_orgs,
                max_orgs_per_user,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
            "#,
            self.settings_row_id,
            deployment_id,
            self.b2b_settings.settings.organizations_enabled,
            self.b2b_settings.settings.workspaces_enabled,
            self.b2b_settings.settings.ip_allowlist_per_org_enabled,
            self.b2b_settings.settings.max_allowed_org_members,
            self.b2b_settings.settings.max_allowed_workspace_members,
            self.b2b_settings.settings.allow_org_deletion,
            self.b2b_settings.settings.allow_workspace_deletion,
            self.b2b_settings.settings.custom_org_role_enabled,
            self.b2b_settings.settings.custom_workspace_role_enabled,
            self.workspace_creator_role_id,
            self.workspace_member_role_id,
            self.org_creator_role_id,
            self.org_member_role_id,
            self.b2b_settings.settings.limit_org_creation_per_user,
            self.b2b_settings.settings.limit_workspace_creation_per_org,
            self.b2b_settings.settings.org_creation_per_user_count,
            self.b2b_settings.settings.workspaces_per_org_count,
            self.b2b_settings.settings.allow_users_to_create_orgs,
            self.b2b_settings.settings.max_orgs_per_user,
            now,
            now,
        )
        .execute(tx.as_mut())
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn insert_workspace_role(
        &self,
        conn: &mut sqlx::PgConnection,
        id: i64,
        deployment_id: i64,
        role: &DeploymentWorkspaceRole,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            INSERT INTO workspace_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            id,
            deployment_id,
            &role.name,
            &role.permissions,
            now,
            now,
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }

    async fn insert_organization_role(
        &self,
        conn: &mut sqlx::PgConnection,
        id: i64,
        deployment_id: i64,
        role: &DeploymentOrganizationRole,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            INSERT INTO organization_roles (
                id,
                deployment_id,
                name,
                permissions,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            id,
            deployment_id,
            &role.name,
            &role.permissions,
            now,
            now,
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

impl DeploymentB2bBootstrapInsertBuilder {
    pub(in crate::project) fn settings_row_id(mut self, settings_row_id: i64) -> Self {
        self.settings_row_id = Some(settings_row_id);
        self
    }

    pub(in crate::project) fn workspace_creator_role_id(mut self, workspace_creator_role_id: i64) -> Self {
        self.workspace_creator_role_id = Some(workspace_creator_role_id);
        self
    }

    pub(in crate::project) fn workspace_member_role_id(mut self, workspace_member_role_id: i64) -> Self {
        self.workspace_member_role_id = Some(workspace_member_role_id);
        self
    }

    pub(in crate::project) fn org_creator_role_id(mut self, org_creator_role_id: i64) -> Self {
        self.org_creator_role_id = Some(org_creator_role_id);
        self
    }

    pub(in crate::project) fn org_member_role_id(mut self, org_member_role_id: i64) -> Self {
        self.org_member_role_id = Some(org_member_role_id);
        self
    }

    pub(in crate::project) fn b2b_settings(mut self, b2b_settings: DeploymentB2bSettingsWithRoles) -> Self {
        self.b2b_settings = Some(b2b_settings);
        self
    }

    pub(in crate::project) fn build(self) -> Result<DeploymentB2bBootstrapInsert, AppError> {
        let settings_row_id = self.settings_row_id.ok_or_else(|| {
            AppError::Validation("deployment_b2b_settings insert id is required".to_string())
        })?;
        let workspace_creator_role_id = self.workspace_creator_role_id.ok_or_else(|| {
            AppError::Validation("workspace creator role id is required".to_string())
        })?;
        let workspace_member_role_id = self.workspace_member_role_id.ok_or_else(|| {
            AppError::Validation("workspace member role id is required".to_string())
        })?;
        let org_creator_role_id = self.org_creator_role_id.ok_or_else(|| {
            AppError::Validation("organization creator role id is required".to_string())
        })?;
        let org_member_role_id = self.org_member_role_id.ok_or_else(|| {
            AppError::Validation("organization member role id is required".to_string())
        })?;
        let b2b_settings = self.b2b_settings.ok_or_else(|| {
            AppError::Validation("deployment_b2b_settings payload is required".to_string())
        })?;

        Ok(DeploymentB2bBootstrapInsert {
            settings_row_id,
            workspace_creator_role_id,
            workspace_member_role_id,
            org_creator_role_id,
            org_member_role_id,
            b2b_settings,
        })
    }
}

