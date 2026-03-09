use super::*;
pub(in crate::project) struct DeploymentSmsTemplatesInsert {
    id: i64,
    sms_templates: DeploymentSmsTemplate,
}

#[derive(Default)]
pub(in crate::project) struct DeploymentSmsTemplatesInsertBuilder {
    id: Option<i64>,
    sms_templates: Option<DeploymentSmsTemplate>,
}

impl DeploymentSmsTemplatesInsert {
    pub(in crate::project) fn builder() -> DeploymentSmsTemplatesInsertBuilder {
        DeploymentSmsTemplatesInsertBuilder::default()
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO deployment_sms_templates (
                id,
                deployment_id,
                reset_password_code_template,
                verification_code_template,
                password_change_template,
                password_remove_template,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            self.id,
            self.sms_templates.deployment_id,
            &self.sms_templates.reset_password_code_template,
            &self.sms_templates.verification_code_template,
            &self.sms_templates.password_change_template,
            &self.sms_templates.password_remove_template,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentSmsTemplatesInsertBuilder {
    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn sms_templates(mut self, sms_templates: DeploymentSmsTemplate) -> Self {
        self.sms_templates = Some(sms_templates);
        self
    }

    pub(in crate::project) fn build(self) -> Result<DeploymentSmsTemplatesInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_sms_templates insert id is required".to_string())
        })?;
        let sms_templates = self.sms_templates.ok_or_else(|| {
            AppError::Validation("deployment_sms_templates payload is required".to_string())
        })?;

        Ok(DeploymentSmsTemplatesInsert { id, sms_templates })
    }
}

