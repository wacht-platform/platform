use crate::Query;
use common::error::AppError;
use common::state::AppState;
use models::plan_features::{PlanFeature, PlanTier};

pub struct CheckDeploymentFeatureAccessQuery {
    pub deployment_id: i64,
    pub feature: PlanFeature,
}

impl CheckDeploymentFeatureAccessQuery {
    pub fn new(deployment_id: i64, feature: PlanFeature) -> Self {
        Self {
            deployment_id,
            feature,
        }
    }
}

impl Query for CheckDeploymentFeatureAccessQuery {
    type Output = bool;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        // Get the project's billing_account_id
        let billing_account_id: Option<i64> = sqlx::query_scalar!(
            r#"
            SELECT billing_account_id
            FROM projects
            WHERE id = (SELECT project_id FROM deployments WHERE id = $1)
            "#,
            self.deployment_id
        )
        .fetch_optional(&state.db_pool)
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
        .fetch_optional(&state.db_pool)
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

pub struct GetDeploymentPlanTierQuery {
    pub deployment_id: i64,
}

impl GetDeploymentPlanTierQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentPlanTierQuery {
    type Output = Option<PlanTier>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        // Get the project's billing_account_id
        let billing_account_id: Option<i64> = sqlx::query_scalar!(
            r#"
            SELECT billing_account_id
            FROM projects
            WHERE id = (SELECT project_id FROM deployments WHERE id = $1)
            "#,
            self.deployment_id
        )
        .fetch_optional(&state.db_pool)
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
        .fetch_optional(&state.db_pool)
        .await?;

        let product_id = match product_id {
            Some(Some(id)) => id,
            _ => return Ok(None),
        };

        Ok(PlanTier::from_product_id(&product_id))
    }
}
