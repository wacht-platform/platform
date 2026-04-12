use super::*;
pub(in crate::project) struct DeploymentAuthSettingsInsert {
    id: i64,
    auth_settings: DeploymentAuthSettings,
}

#[derive(Default)]
pub(in crate::project) struct DeploymentAuthSettingsInsertBuilder {
    id: Option<i64>,
    auth_settings: Option<DeploymentAuthSettings>,
}

impl DeploymentAuthSettingsInsert {
    pub(in crate::project) fn builder() -> DeploymentAuthSettingsInsertBuilder {
        DeploymentAuthSettingsInsertBuilder::default()
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
            INSERT INTO deployment_auth_settings (
                id,
                deployment_id,
                email_address,
                phone_number,
                username,
                first_name,
                last_name,
                password,
                magic_link,
                passkey,
                auth_factors_enabled,
                verification_policy,
                second_factor_policy,
                first_factor,
                multi_session_support,
                session_token_lifetime,
                session_validity_period,
                session_inactive_timeout,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
            "#,
            self.id,
            self.auth_settings.deployment_id,
            json_value(&self.auth_settings.email_address)?,
            json_value(&self.auth_settings.phone_number)?,
            json_value(&self.auth_settings.username)?,
            json_value(&self.auth_settings.first_name)?,
            json_value(&self.auth_settings.last_name)?,
            json_value(&self.auth_settings.password)?,
            json_value(&self.auth_settings.magic_link)?,
            json_value(&self.auth_settings.passkey)?,
            json_value(&self.auth_settings.auth_factors_enabled)?,
            json_value(&self.auth_settings.verification_policy)?,
            self.auth_settings.second_factor_policy.to_string(),
            self.auth_settings.first_factor.to_string(),
            json_value(&self.auth_settings.multi_session_support)?,
            self.auth_settings.session_token_lifetime,
            self.auth_settings.session_validity_period,
            self.auth_settings.session_inactive_timeout,
            now,
            now
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentAuthSettingsInsertBuilder {
    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn auth_settings(
        mut self,
        auth_settings: DeploymentAuthSettings,
    ) -> Self {
        self.auth_settings = Some(auth_settings);
        self
    }

    pub(in crate::project) fn build(self) -> Result<DeploymentAuthSettingsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_auth_settings insert id is required".to_string())
        })?;
        let auth_settings = self.auth_settings.ok_or_else(|| {
            AppError::Validation("deployment_auth_settings payload is required".to_string())
        })?;

        Ok(DeploymentAuthSettingsInsert { id, auth_settings })
    }
}
