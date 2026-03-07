use common::error::AppError;
use models::plan_features::{PlanFeature, PlanTier};

pub struct CheckDeploymentFeatureAccessQuery {
    deployment_id: i64,
    feature: PlanFeature,
}

#[derive(Default)]
pub struct CheckDeploymentFeatureAccessQueryBuilder {
    deployment_id: Option<i64>,
    feature: Option<PlanFeature>,
}

impl CheckDeploymentFeatureAccessQuery {
    pub fn builder() -> CheckDeploymentFeatureAccessQueryBuilder {
        CheckDeploymentFeatureAccessQueryBuilder::default()
    }

    pub fn new(deployment_id: i64, feature: PlanFeature) -> Self {
        Self {
            deployment_id,
            feature,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<bool, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let product_id: Option<Option<String>> = sqlx::query_scalar(
            r#"
            SELECT s.product_id
            FROM deployments d
            JOIN projects p ON p.id = d.project_id
            JOIN subscriptions s
              ON s.billing_account_id = p.billing_account_id
             AND s.status = 'active'
            WHERE d.id = $1
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(executor)
        .await?;

        let Some(Some(product_id)) = product_id else {
            return Ok(false);
        };

        Ok(PlanTier::from_product_id(&product_id)
            .map(|tier| tier.has_feature(self.feature))
            .unwrap_or(false))
    }
}

impl CheckDeploymentFeatureAccessQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn feature(mut self, feature: PlanFeature) -> Self {
        self.feature = Some(feature);
        self
    }

    pub fn build(self) -> Result<CheckDeploymentFeatureAccessQuery, AppError> {
        Ok(CheckDeploymentFeatureAccessQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            feature: self
                .feature
                .ok_or_else(|| AppError::Validation("feature is required".into()))?,
        })
    }
}

pub struct GetDeploymentPlanTierQuery {
    deployment_id: i64,
}

#[derive(Default)]
pub struct GetDeploymentPlanTierQueryBuilder {
    deployment_id: Option<i64>,
}

impl GetDeploymentPlanTierQuery {
    pub fn builder() -> GetDeploymentPlanTierQueryBuilder {
        GetDeploymentPlanTierQueryBuilder::default()
    }

    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<PlanTier>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let product_id: Option<Option<String>> = sqlx::query_scalar(
            r#"
            SELECT s.product_id
            FROM deployments d
            JOIN projects p ON p.id = d.project_id
            JOIN subscriptions s
              ON s.billing_account_id = p.billing_account_id
             AND s.status = 'active'
            WHERE d.id = $1
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(executor)
        .await?;

        let Some(Some(product_id)) = product_id else {
            return Ok(None);
        };

        Ok(PlanTier::from_product_id(&product_id))
    }
}

impl GetDeploymentPlanTierQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn build(self) -> Result<GetDeploymentPlanTierQuery, AppError> {
        Ok(GetDeploymentPlanTierQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
        })
    }
}
