use super::*;

#[derive(Default)]
pub(in crate::project) struct DeploymentEmailVerificationUpdate {
    deployment_id: Option<i64>,
    email_verification_records: Option<serde_json::Value>,
}

impl DeploymentEmailVerificationUpdate {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(in crate::project) fn email_verification_records(
        mut self,
        email_verification_records: serde_json::Value,
    ) -> Self {
        self.email_verification_records = Some(email_verification_records);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;
        let email_verification_records =
            self.email_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation("email_verification_records are required".to_string())
            })?;

        sqlx::query!(
            r#"
            UPDATE deployments
            SET email_verification_records = $1, updated_at = $2
            WHERE id = $3
            "#,
            email_verification_records,
            chrono::Utc::now(),
            deployment_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(in crate::project) struct DeploymentDomainVerificationUpdate {
    deployment_id: Option<i64>,
    domain_verification_records: Option<serde_json::Value>,
}

impl DeploymentDomainVerificationUpdate {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(in crate::project) fn domain_verification_records(
        mut self,
        domain_verification_records: serde_json::Value,
    ) -> Self {
        self.domain_verification_records = Some(domain_verification_records);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;
        let domain_verification_records =
            self.domain_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation("domain_verification_records are required".to_string())
            })?;

        sqlx::query!(
            r#"
            UPDATE deployments
            SET domain_verification_records = $1, updated_at = $2
            WHERE id = $3
            "#,
            domain_verification_records,
            chrono::Utc::now(),
            deployment_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Default)]
pub(in crate::project) struct DeploymentDnsRecordsUpdate {
    deployment_id: Option<i64>,
    domain_verification_records: Option<serde_json::Value>,
    email_verification_records: Option<serde_json::Value>,
}

impl DeploymentDnsRecordsUpdate {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(in crate::project) fn domain_verification_records(
        mut self,
        domain_verification_records: serde_json::Value,
    ) -> Self {
        self.domain_verification_records = Some(domain_verification_records);
        self
    }

    pub(in crate::project) fn email_verification_records(
        mut self,
        email_verification_records: serde_json::Value,
    ) -> Self {
        self.email_verification_records = Some(email_verification_records);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;
        let domain_verification_records =
            self.domain_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation("domain_verification_records are required".to_string())
            })?;
        let email_verification_records =
            self.email_verification_records.as_ref().ok_or_else(|| {
                AppError::Validation("email_verification_records are required".to_string())
            })?;

        sqlx::query!(
            r#"
            UPDATE deployments
            SET domain_verification_records = $1,
                email_verification_records = $2,
                updated_at = $3
            WHERE id = $4
            "#,
            domain_verification_records,
            email_verification_records,
            chrono::Utc::now(),
            deployment_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}
