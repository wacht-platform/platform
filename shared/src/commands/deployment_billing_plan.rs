use crate::{
    error::AppError,
    models::{DeploymentBillingPlan, BillingInterval, BillingUsageType},
    state::AppState,
};
use serde_json::Value;

use super::Command;

pub struct CreateDeploymentBillingPlanCommand {
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub stripe_price_id: Option<String>,
    pub billing_interval: BillingInterval,
    pub amount_cents: i64,
    pub currency: String,
    pub trial_period_days: Option<i32>,
    pub usage_type: Option<BillingUsageType>,
    pub features: Option<Value>,
    pub is_active: bool,
    pub display_order: i32,
}

impl Command for CreateDeploymentBillingPlanCommand {
    type Output = DeploymentBillingPlan;

    async fn execute(self, app_state: &AppState) -> Result<DeploymentBillingPlan, AppError> {
        let plan = sqlx::query_as!(
            DeploymentBillingPlan,
            r#"
            INSERT INTO deployment_billing_plans (
                deployment_id,
                name,
                description,
                stripe_price_id,
                billing_interval,
                amount_cents,
                currency,
                trial_period_days,
                usage_type,
                features,
                is_active,
                display_order
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING 
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
            "#,
            self.deployment_id,
            self.name,
            self.description,
            self.stripe_price_id,
            self.billing_interval as BillingInterval,
            self.amount_cents,
            self.currency,
            self.trial_period_days,
            self.usage_type as Option<BillingUsageType>,
            self.features,
            self.is_active,
            self.display_order
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.constraint() == Some("deployment_billing_plans_deployment_id_fkey") => {
                AppError::BadRequest("Invalid deployment ID".to_string())
            }
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(plan)
    }
}

pub struct UpdateDeploymentBillingPlanCommand {
    pub deployment_id: i64,
    pub plan_id: i64,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub trial_period_days: Option<Option<i32>>,
    pub features: Option<Option<Value>>,
    pub is_active: Option<bool>,
    pub display_order: Option<i32>,
}

impl Command for UpdateDeploymentBillingPlanCommand {
    type Output = DeploymentBillingPlan;

    async fn execute(self, app_state: &AppState) -> Result<DeploymentBillingPlan, AppError> {
        let plan = sqlx::query_as!(
            DeploymentBillingPlan,
            r#"
            UPDATE deployment_billing_plans 
            SET 
                updated_at = NOW(),
                name = COALESCE($3, name),
                description = COALESCE($4, description),
                trial_period_days = COALESCE($5, trial_period_days),
                features = COALESCE($6, features),
                is_active = COALESCE($7, is_active),
                display_order = COALESCE($8, display_order)
            WHERE deployment_id = $1 AND id = $2
            RETURNING 
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
            "#,
            self.deployment_id,
            self.plan_id,
            self.name,
            self.description.flatten(),
            self.trial_period_days.flatten(),
            self.features.flatten(),
            self.is_active,
            self.display_order
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

pub struct DeleteDeploymentBillingPlanCommand {
    pub deployment_id: i64,
    pub plan_id: i64,
}

impl Command for DeleteDeploymentBillingPlanCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<(), AppError> {
        // First check if there are active subscriptions using this plan
        let active_subscriptions = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM deployment_subscriptions 
             WHERE deployment_id = $1 AND billing_plan_id = $2 
             AND status IN ('active', 'trialing', 'past_due')",
            self.deployment_id,
            self.plan_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        if active_subscriptions.unwrap_or(0) > 0 {
            return Err(AppError::BadRequest(
                "Cannot delete plan with active subscriptions".to_string()
            ));
        }

        // Soft delete by setting is_active to false
        let result = sqlx::query!(
            "UPDATE deployment_billing_plans 
             SET is_active = false, updated_at = NOW() 
             WHERE deployment_id = $1 AND id = $2",
            self.deployment_id,
            self.plan_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Billing plan not found".to_string()));
        }

        Ok(())
    }
}