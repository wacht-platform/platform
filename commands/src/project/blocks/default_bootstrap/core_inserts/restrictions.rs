use super::*;
pub(in crate::project) struct DeploymentRestrictionsInsert {
    id: i64,
    restrictions: DeploymentRestrictions,
}

#[derive(Default)]
pub(in crate::project) struct DeploymentRestrictionsInsertBuilder {
    id: Option<i64>,
    restrictions: Option<DeploymentRestrictions>,
}

impl DeploymentRestrictionsInsert {
    pub(in crate::project) fn builder() -> DeploymentRestrictionsInsertBuilder {
        DeploymentRestrictionsInsertBuilder::default()
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO deployment_restrictions (
                id,
                deployment_id,
                allowlist_enabled,
                blocklist_enabled,
                block_subaddresses,
                block_disposable_emails,
                block_voip_numbers,
                country_restrictions,
                banned_keywords,
                allowlisted_resources,
                blocklisted_resources,
                sign_up_mode,
                waitlist_collect_names,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            self.id,
            self.restrictions.deployment_id,
            self.restrictions.allowlist_enabled,
            self.restrictions.blocklist_enabled,
            self.restrictions.block_subaddresses,
            self.restrictions.block_disposable_emails,
            self.restrictions.block_voip_numbers,
            json_value(&self.restrictions.country_restrictions)?,
            &self.restrictions.banned_keywords,
            &self.restrictions.allowlisted_resources,
            &self.restrictions.blocklisted_resources,
            self.restrictions.sign_up_mode.to_string(),
            self.restrictions.waitlist_collect_names,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentRestrictionsInsertBuilder {
    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn restrictions(mut self, restrictions: DeploymentRestrictions) -> Self {
        self.restrictions = Some(restrictions);
        self
    }

    pub(in crate::project) fn build(self) -> Result<DeploymentRestrictionsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_restrictions insert id is required".to_string())
        })?;
        let restrictions = self.restrictions.ok_or_else(|| {
            AppError::Validation("deployment_restrictions payload is required".to_string())
        })?;

        Ok(DeploymentRestrictionsInsert { id, restrictions })
    }
}

