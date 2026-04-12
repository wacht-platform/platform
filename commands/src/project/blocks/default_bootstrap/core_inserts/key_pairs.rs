use super::*;
pub(in crate::project) struct DeploymentKeyPairsInsert {
    id: i64,
    deployment_id: i64,
    public_key: String,
    private_key: String,
    saml_public_key: String,
    saml_private_key: String,
}

#[derive(Default)]
pub(in crate::project) struct DeploymentKeyPairsInsertBuilder {
    id: Option<i64>,
    deployment_id: Option<i64>,
    public_key: Option<String>,
    private_key: Option<String>,
    saml_public_key: Option<String>,
    saml_private_key: Option<String>,
}

impl DeploymentKeyPairsInsert {
    pub(in crate::project) fn builder() -> DeploymentKeyPairsInsertBuilder {
        DeploymentKeyPairsInsertBuilder::default()
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO deployment_key_pairs (
                id,
                deployment_id,
                public_key,
                private_key,
                saml_public_key,
                saml_private_key,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            self.id,
            self.deployment_id,
            &self.public_key,
            &self.private_key,
            &self.saml_public_key,
            &self.saml_private_key,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentKeyPairsInsertBuilder {
    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(in crate::project) fn public_key(mut self, public_key: String) -> Self {
        self.public_key = Some(public_key);
        self
    }

    pub(in crate::project) fn private_key(mut self, private_key: String) -> Self {
        self.private_key = Some(private_key);
        self
    }

    pub(in crate::project) fn saml_public_key(mut self, saml_public_key: String) -> Self {
        self.saml_public_key = Some(saml_public_key);
        self
    }

    pub(in crate::project) fn saml_private_key(mut self, saml_private_key: String) -> Self {
        self.saml_private_key = Some(saml_private_key);
        self
    }

    pub(in crate::project) fn build(self) -> Result<DeploymentKeyPairsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs insert id is required".to_string())
        })?;
        let deployment_id = self.deployment_id.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs deployment_id is required".to_string())
        })?;
        let public_key = self.public_key.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs public_key is required".to_string())
        })?;
        let private_key = self.private_key.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs private_key is required".to_string())
        })?;
        let saml_public_key = self.saml_public_key.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs saml_public_key is required".to_string())
        })?;
        let saml_private_key = self.saml_private_key.ok_or_else(|| {
            AppError::Validation("deployment_key_pairs saml_private_key is required".to_string())
        })?;

        Ok(DeploymentKeyPairsInsert {
            id,
            deployment_id,
            public_key,
            private_key,
            saml_public_key,
            saml_private_key,
        })
    }
}
