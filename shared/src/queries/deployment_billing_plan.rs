use crate::{
    error::AppError,
    models::DeploymentBillingPlan,
    state::AppState,
};

use super::Query;

pub struct GetDeploymentBillingPlansQuery {
    deployment_id: i64,
    active_only: bool,
    limit: Option<i32>,
    offset: Option<i32>,
}

impl GetDeploymentBillingPlansQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            active_only: false,
            limit: None,
            offset: None,
        }
    }

    pub fn active_only(mut self, active_only: bool) -> Self {
        self.active_only = active_only;
        self
    }

    pub fn with_limit(mut self, limit: Option<i32>) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_offset(mut self, offset: Option<i32>) -> Self {
        self.offset = offset;
        self
    }
}

impl Query for GetDeploymentBillingPlansQuery {
    type Output = Vec<DeploymentBillingPlan>;

    async fn execute(&self, app_state: &AppState) -> Result<Vec<DeploymentBillingPlan>, AppError> {
        if self.active_only {
            let plans = sqlx::query_as!(
                DeploymentBillingPlan,
                r#"
                SELECT 
                    id,
                    created_at,
                    updated_at,
                    deployment_id,
                    name,
                    description,
                    stripe_price_id,
                    billing_interval as "billing_interval!: crate::models::BillingInterval",
                    amount_cents,
                    currency,
                    trial_period_days,
                    usage_type as "usage_type: crate::models::BillingUsageType",
                    features,
                    is_active,
                    display_order as "display_order!"
                FROM deployment_billing_plans 
                WHERE deployment_id = $1 AND is_active = true
                ORDER BY display_order ASC, created_at DESC
                LIMIT $2
                OFFSET $3
                "#,
                self.deployment_id,
                self.limit.unwrap_or(100) as i64,
                self.offset.unwrap_or(0) as i64
            )
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

            Ok(plans)
        } else {
            let plans = sqlx::query_as!(
                DeploymentBillingPlan,
                r#"
                SELECT 
                    id,
                    created_at,
                    updated_at,
                    deployment_id,
                    name,
                    description,
                    stripe_price_id,
                    billing_interval as "billing_interval!: crate::models::BillingInterval",
                    amount_cents,
                    currency,
                    trial_period_days,
                    usage_type as "usage_type: crate::models::BillingUsageType",
                    features,
                    is_active,
                    display_order as "display_order!"
                FROM deployment_billing_plans 
                WHERE deployment_id = $1
                ORDER BY display_order ASC, created_at DESC
                LIMIT $2
                OFFSET $3
                "#,
                self.deployment_id,
                self.limit.unwrap_or(100) as i64,
                self.offset.unwrap_or(0) as i64
            )
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

            Ok(plans)
        }
    }
}

pub struct GetDeploymentBillingPlanByIdQuery {
    deployment_id: i64,
    plan_id: i64,
}

impl GetDeploymentBillingPlanByIdQuery {
    pub fn new(deployment_id: i64, plan_id: i64) -> Self {
        Self {
            deployment_id,
            plan_id,
        }
    }
}

impl Query for GetDeploymentBillingPlanByIdQuery {
    type Output = DeploymentBillingPlan;

    async fn execute(&self, app_state: &AppState) -> Result<DeploymentBillingPlan, AppError> {
        let plan = sqlx::query_as!(
            DeploymentBillingPlan,
            r#"
            SELECT 
                id,
                created_at,
                updated_at,
                deployment_id,
                name,
                description,
                stripe_price_id,
                billing_interval as "billing_interval!: crate::models::BillingInterval",
                amount_cents,
                currency,
                trial_period_days,
                usage_type as "usage_type: crate::models::BillingUsageType",
                features,
                is_active,
                display_order as "display_order!"
            FROM deployment_billing_plans 
            WHERE deployment_id = $1 AND id = $2
            "#,
            self.deployment_id,
            self.plan_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Billing plan not found".to_string()),
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(plan)
    }
}

pub struct GetDeploymentBillingPlanByStripeIdQuery {
    deployment_id: i64,
    stripe_price_id: String,
}

impl GetDeploymentBillingPlanByStripeIdQuery {
    pub fn new(deployment_id: i64, stripe_price_id: String) -> Self {
        Self {
            deployment_id,
            stripe_price_id,
        }
    }
}

impl Query for GetDeploymentBillingPlanByStripeIdQuery {
    type Output = DeploymentBillingPlan;

    async fn execute(&self, app_state: &AppState) -> Result<DeploymentBillingPlan, AppError> {
        let plan = sqlx::query_as!(
            DeploymentBillingPlan,
            r#"
            SELECT 
                id,
                created_at,
                updated_at,
                deployment_id,
                name,
                description,
                stripe_price_id,
                billing_interval as "billing_interval!: crate::models::BillingInterval",
                amount_cents,
                currency,
                trial_period_days,
                usage_type as "usage_type: crate::models::BillingUsageType",
                features,
                is_active,
                display_order as "display_order!"
            FROM deployment_billing_plans 
            WHERE deployment_id = $1 AND stripe_price_id = $2
            "#,
            self.deployment_id,
            self.stripe_price_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Billing plan not found".to_string()),
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(plan)
    }
}