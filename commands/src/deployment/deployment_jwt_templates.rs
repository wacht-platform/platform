use chrono::Utc;
use sqlx::Row;

use common::{HasDbRouter, HasRedis, error::AppError};
use dto::json::{NewDeploymentJwtTemplate, PartialDeploymentJwtTemplate};
use models::DeploymentJwtTemplate;

use super::ClearDeploymentCacheCommand;

pub struct CreateDeploymentJwtTemplateCommand {
    pub template_id: Option<i64>,
    pub deployment_id: i64,
    pub template: NewDeploymentJwtTemplate,
}

impl CreateDeploymentJwtTemplateCommand {
    pub fn new(deployment_id: i64, template: NewDeploymentJwtTemplate) -> Self {
        Self {
            template_id: None,
            deployment_id,
            template,
        }
    }

    pub fn with_template_id(mut self, template_id: i64) -> Self {
        self.template_id = Some(template_id);
        self
    }
}

impl CreateDeploymentJwtTemplateCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<DeploymentJwtTemplate, AppError>
    where
        D: HasDbRouter + HasRedis,
    {
        let mut conn = deps.db_router().writer().acquire().await?;
        let template_id = self
            .template_id
            .ok_or_else(|| AppError::Validation("template_id is required".to_string()))?;
        let result = sqlx::query!(
            r#"
            INSERT INTO deployment_jwt_templates (id, created_at, updated_at, deployment_id, name, token_lifetime, allowed_clock_skew, custom_signing_key, template)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
            "#,
            template_id,
            Utc::now(),
            Utc::now(),
            self.deployment_id,
            self.template.name,
            self.template.token_lifetime,
            self.template.allowed_clock_skew,
            serde_json::to_value(self.template.custom_signing_key)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            self.template.template,
        )
        .fetch_one(&mut *conn)
        .await?;

        let template = DeploymentJwtTemplate {
            id: result.id,
            created_at: result.created_at,
            updated_at: result.updated_at,
            deployment_id: result.deployment_id,
            name: result.name,
            token_lifetime: result.token_lifetime,
            allowed_clock_skew: result.allowed_clock_skew,
            custom_signing_key: result
                .custom_signing_key
                .map(|v| serde_json::from_value(v).unwrap_or_default()),
            template: serde_json::from_value(result.template).unwrap_or_default(),
        };

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;

        Ok(template)
    }
}

pub struct UpdateDeploymentJwtTemplateCommand {
    pub deployment_id: i64,
    pub id: i64,
    pub template: PartialDeploymentJwtTemplate,
}

impl UpdateDeploymentJwtTemplateCommand {
    pub fn new(deployment_id: i64, id: i64, template: PartialDeploymentJwtTemplate) -> Self {
        Self {
            deployment_id,
            id,
            template,
        }
    }
}

impl UpdateDeploymentJwtTemplateCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<DeploymentJwtTemplate, AppError>
    where
        D: HasDbRouter + HasRedis,
    {
        let mut conn = deps.db_router().writer().acquire().await?;
        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_jwt_templates SET updated_at = NOW() ");

        if let Some(name) = &self.template.name {
            query_builder.push(", name = ");
            query_builder.push_bind(name);
        }

        if let Some(token_lifetime) = &self.template.token_lifetime {
            query_builder.push(", token_lifetime = ");
            query_builder.push_bind(token_lifetime);
        }

        if let Some(allowed_clock_skew) = &self.template.allowed_clock_skew {
            query_builder.push(", allowed_clock_skew = ");
            query_builder.push_bind(allowed_clock_skew);
        }

        query_builder.push(", custom_signing_key = ");
        query_builder.push_bind(
            serde_json::to_value(&self.template.custom_signing_key)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
        );

        if let Some(template) = &self.template.template {
            query_builder.push(", template = ");
            query_builder.push_bind(
                serde_json::to_value(template)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            );
        }

        query_builder.push(" WHERE id = ");
        query_builder.push_bind(self.id);
        query_builder.push(" AND deployment_id = ");
        query_builder.push_bind(self.deployment_id);

        query_builder.push(" RETURNING *");

        let result = query_builder
            .build()
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "JWT template {} not found in deployment {}",
                    self.id, self.deployment_id
                ))
            })?;

        let template = DeploymentJwtTemplate {
            id: result.get("id"),
            created_at: result.get("created_at"),
            updated_at: result.get("updated_at"),
            deployment_id: result.get("deployment_id"),
            name: result.get("name"),
            token_lifetime: result.get("token_lifetime"),
            allowed_clock_skew: result.get("allowed_clock_skew"),
            custom_signing_key: serde_json::from_value(result.get("custom_signing_key"))
                .unwrap_or_default(),
            template: result.get("template"),
        };

        let deployment_id: i64 = result.get("deployment_id");
        ClearDeploymentCacheCommand::new(deployment_id)
            .execute_with_deps(deps)
            .await?;

        Ok(template)
    }
}

pub struct DeleteDeploymentJwtTemplateCommand {
    pub deployment_id: i64,
    pub id: i64,
}

impl DeleteDeploymentJwtTemplateCommand {
    pub fn new(deployment_id: i64, id: i64) -> Self {
        Self { deployment_id, id }
    }
}

impl DeleteDeploymentJwtTemplateCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasRedis,
    {
        let mut conn = deps.db_router().writer().acquire().await?;
        let result = sqlx::query!(
            "DELETE FROM deployment_jwt_templates WHERE id = $1 AND deployment_id = $2",
            self.id,
            self.deployment_id
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "JWT template {} not found in deployment {}",
                self.id, self.deployment_id
            )));
        }

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;

        Ok(())
    }
}
