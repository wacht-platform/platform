use super::*;

#[derive(Default)]
pub(in crate::project) struct DeleteDeploymentSocialConnectionsByIds {
    deployment_ids: Option<Vec<i64>>,
}

impl DeleteDeploymentSocialConnectionsByIds {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn deployment_ids(mut self, deployment_ids: Vec<i64>) -> Self {
        self.deployment_ids = Some(deployment_ids);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_ids = self
            .deployment_ids
            .as_ref()
            .ok_or_else(|| AppError::Validation("deployment_ids are required".to_string()))?;

        if deployment_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"
            DELETE FROM deployment_social_connections
            WHERE deployment_id = ANY($1::bigint[])
            "#,
            deployment_ids
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(in crate::project) struct DeleteDeploymentAuthSettingsByIds {
    deployment_ids: Option<Vec<i64>>,
}

impl DeleteDeploymentAuthSettingsByIds {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn deployment_ids(mut self, deployment_ids: Vec<i64>) -> Self {
        self.deployment_ids = Some(deployment_ids);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_ids = self
            .deployment_ids
            .as_ref()
            .ok_or_else(|| AppError::Validation("deployment_ids are required".to_string()))?;

        if deployment_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"
            DELETE FROM deployment_auth_settings
            WHERE deployment_id = ANY($1::bigint[])
            "#,
            deployment_ids
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(in crate::project) struct DeleteDeploymentUiSettingsByIds {
    deployment_ids: Option<Vec<i64>>,
}

impl DeleteDeploymentUiSettingsByIds {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn deployment_ids(mut self, deployment_ids: Vec<i64>) -> Self {
        self.deployment_ids = Some(deployment_ids);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_ids = self
            .deployment_ids
            .as_ref()
            .ok_or_else(|| AppError::Validation("deployment_ids are required".to_string()))?;

        if deployment_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"
            DELETE FROM deployment_ui_settings
            WHERE deployment_id = ANY($1::bigint[])
            "#,
            deployment_ids
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(in crate::project) struct DeleteDeploymentB2bSettingsByIds {
    deployment_ids: Option<Vec<i64>>,
}

impl DeleteDeploymentB2bSettingsByIds {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn deployment_ids(mut self, deployment_ids: Vec<i64>) -> Self {
        self.deployment_ids = Some(deployment_ids);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_ids = self
            .deployment_ids
            .as_ref()
            .ok_or_else(|| AppError::Validation("deployment_ids are required".to_string()))?;

        if deployment_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"
            DELETE FROM deployment_b2b_settings
            WHERE deployment_id = ANY($1::bigint[])
            "#,
            deployment_ids
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(in crate::project) struct DeleteDeploymentsByProject {
    project_id: Option<i64>,
}

impl DeleteDeploymentsByProject {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        sqlx::query!(
            r#"
            DELETE FROM deployments
            WHERE project_id = $1
            "#,
            project_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(in crate::project) struct DeleteProjectById {
    project_id: Option<i64>,
}

impl DeleteProjectById {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        sqlx::query!(
            r#"
            DELETE FROM projects
            WHERE id = $1
            "#,
            project_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}
