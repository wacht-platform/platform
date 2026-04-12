use super::*;
pub(in crate::project) struct DeploymentEmailTemplatesInsert {
    id: i64,
    email_templates: DeploymentEmailTemplate,
}

#[derive(Default)]
pub(in crate::project) struct DeploymentEmailTemplatesInsertBuilder {
    id: Option<i64>,
    email_templates: Option<DeploymentEmailTemplate>,
}

impl DeploymentEmailTemplatesInsert {
    pub(in crate::project) fn builder() -> DeploymentEmailTemplatesInsertBuilder {
        DeploymentEmailTemplatesInsertBuilder::default()
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
            INSERT INTO deployment_email_templates (
                id,
                deployment_id,
                organization_invite_template,
                verification_code_template,
                reset_password_code_template,
                primary_email_change_template,
                password_change_template,
                password_remove_template,
                sign_in_from_new_device_template,
                magic_link_template,
                waitlist_signup_template,
                waitlist_invite_template,
                workspace_invite_template,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
            self.id,
            self.email_templates.deployment_id,
            json_value(&self.email_templates.organization_invite_template)?,
            json_value(&self.email_templates.verification_code_template)?,
            json_value(&self.email_templates.reset_password_code_template)?,
            json_value(&self.email_templates.primary_email_change_template)?,
            json_value(&self.email_templates.password_change_template)?,
            json_value(&self.email_templates.password_remove_template)?,
            json_value(&self.email_templates.sign_in_from_new_device_template)?,
            json_value(&self.email_templates.magic_link_template)?,
            json_value(&self.email_templates.waitlist_signup_template)?,
            json_value(&self.email_templates.waitlist_invite_template)?,
            json_value(&self.email_templates.workspace_invite_template)?,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentEmailTemplatesInsertBuilder {
    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn email_templates(
        mut self,
        email_templates: DeploymentEmailTemplate,
    ) -> Self {
        self.email_templates = Some(email_templates);
        self
    }

    pub(in crate::project) fn build(self) -> Result<DeploymentEmailTemplatesInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_email_templates insert id is required".to_string())
        })?;
        let email_templates = self.email_templates.ok_or_else(|| {
            AppError::Validation("deployment_email_templates payload is required".to_string())
        })?;

        Ok(DeploymentEmailTemplatesInsert {
            id,
            email_templates,
        })
    }
}
