use crate::Query;
use common::error::AppError;
use common::state::AppState;
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

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        // Get the project's billing_account_id
        let billing_account_id: Option<i64> = sqlx::query_scalar!(
            r#"
            SELECT billing_account_id
            FROM projects
            WHERE id = (SELECT project_id FROM deployments WHERE id = $1)
            "#,
            self.deployment_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        let billing_account_id = match billing_account_id {
            Some(id) => id,
            None => return Ok(false), // No billing account = no access
        };

        // Get the subscription's product_id
        let product_id = sqlx::query_scalar!(
            r#"
            SELECT product_id
            FROM subscriptions
            WHERE billing_account_id = $1 AND status = 'active'
            "#,
            billing_account_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        let product_id = match product_id {
            Some(Some(id)) => id,
            _ => return Ok(false), // No active subscription or null product_id = no access
        };

        // Map product_id to plan tier and check feature access
        let plan_tier = PlanTier::from_product_id(&product_id);

        match plan_tier {
            Some(tier) => Ok(tier.has_feature(self.feature)),
            None => Ok(false), // Unknown plan = no access
        }
    }
}

impl Query for CheckDeploymentFeatureAccessQuery {
    type Output = bool;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(state.db_router.writer()).await
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

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Option<PlanTier>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        // Get the project's billing_account_id
        let billing_account_id: Option<i64> = sqlx::query_scalar!(
            r#"
            SELECT billing_account_id
            FROM projects
            WHERE id = (SELECT project_id FROM deployments WHERE id = $1)
            "#,
            self.deployment_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        let billing_account_id = match billing_account_id {
            Some(id) => id,
            None => return Ok(None),
        };

        // Get the subscription's product_id
        let product_id = sqlx::query_scalar!(
            r#"
            SELECT product_id
            FROM subscriptions
            WHERE billing_account_id = $1 AND status = 'active'
            "#,
            billing_account_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        let product_id = match product_id {
            Some(Some(id)) => id,
            _ => return Ok(None),
        };

        Ok(PlanTier::from_product_id(&product_id))
    }
}

impl Query for GetDeploymentPlanTierQuery {
    type Output = Option<PlanTier>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(state.db_router.writer()).await
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
