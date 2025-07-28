use crate::{
    error::AppError,
    models::{DeploymentSubscription, DeploymentSubscriptionWithPlan, DeploymentBillingPlan, SubscriptionStatus},
    state::AppState,
};

use super::Query;

pub struct GetDeploymentSubscriptionsQuery {
    deployment_id: i64,
    user_id: Option<i64>,
    status: Option<SubscriptionStatus>,
    limit: Option<i32>,
    offset: Option<i32>,
}

impl GetDeploymentSubscriptionsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            user_id: None,
            status: None,
            limit: None,
            offset: None,
        }
    }

    pub fn for_user(mut self, user_id: i64) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_status(mut self, status: SubscriptionStatus) -> Self {
        self.status = Some(status);
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

impl Query for GetDeploymentSubscriptionsQuery {
    type Output = Vec<DeploymentSubscriptionWithPlan>;

    async fn execute(&self, app_state: &AppState) -> Result<Vec<DeploymentSubscriptionWithPlan>, AppError> {
        // Use a simpler approach with just the subscription query
        let subscriptions = sqlx::query_as!(
            DeploymentSubscription,
            r#"
            SELECT 
                id,
                created_at,
                updated_at,
                deployment_id,
                user_id,
                stripe_subscription_id,
                stripe_customer_id,
                billing_plan_id,
                status as "status!: crate::models::SubscriptionStatus",
                current_period_start,
                current_period_end,
                trial_start,
                trial_end,
                cancel_at_period_end,
                canceled_at,
                ended_at,
                collection_method as "collection_method!: crate::models::CollectionMethod",
                customer_email,
                customer_name,
                metadata
            FROM deployment_subscriptions
            WHERE deployment_id = $1
              AND ($2::BIGINT IS NULL OR user_id = $2)
              AND ($3::TEXT IS NULL OR status = $3)
            ORDER BY created_at DESC
            LIMIT $4
            OFFSET $5
            "#,
            self.deployment_id,
            self.user_id,
            self.status.as_ref().map(|s| s.to_string()),
            self.limit.unwrap_or(100) as i64,
            self.offset.unwrap_or(0) as i64
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        // Load billing plans for subscriptions with billing_plan_id
        let mut results = Vec::new();
        for subscription in subscriptions {
            let billing_plan = if let Some(plan_id) = subscription.billing_plan_id {
                sqlx::query_as!(
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
                    WHERE id = $1
                    "#,
                    plan_id
                )
                .fetch_optional(&app_state.db_pool)
                .await?
            } else {
                None
            };

            results.push(DeploymentSubscriptionWithPlan {
                subscription,
                billing_plan,
            });
        }

        Ok(results)
    }
}

pub struct GetDeploymentSubscriptionByIdQuery {
    deployment_id: i64,
    subscription_id: i64,
}

impl GetDeploymentSubscriptionByIdQuery {
    pub fn new(deployment_id: i64, subscription_id: i64) -> Self {
        Self {
            deployment_id,
            subscription_id,
        }
    }
}

impl Query for GetDeploymentSubscriptionByIdQuery {
    type Output = DeploymentSubscription;

    async fn execute(&self, app_state: &AppState) -> Result<DeploymentSubscription, AppError> {
        let subscription = sqlx::query_as!(
            DeploymentSubscription,
            r#"
            SELECT 
                id,
                created_at,
                updated_at,
                deployment_id,
                user_id,
                stripe_subscription_id,
                stripe_customer_id,
                billing_plan_id,
                status as "status!: crate::models::SubscriptionStatus",
                current_period_start,
                current_period_end,
                trial_start,
                trial_end,
                cancel_at_period_end,
                canceled_at,
                ended_at,
                collection_method as "collection_method!: crate::models::CollectionMethod",
                customer_email,
                customer_name,
                metadata
            FROM deployment_subscriptions 
            WHERE deployment_id = $1 AND id = $2
            "#,
            self.deployment_id,
            self.subscription_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Subscription not found".to_string()),
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(subscription)
    }
}

pub struct GetDeploymentSubscriptionByStripeIdQuery {
    stripe_subscription_id: String,
}

impl GetDeploymentSubscriptionByStripeIdQuery {
    pub fn new(stripe_subscription_id: String) -> Self {
        Self {
            stripe_subscription_id,
        }
    }
}

impl Query for GetDeploymentSubscriptionByStripeIdQuery {
    type Output = DeploymentSubscription;

    async fn execute(&self, app_state: &AppState) -> Result<DeploymentSubscription, AppError> {
        let subscription = sqlx::query_as!(
            DeploymentSubscription,
            r#"
            SELECT 
                id,
                created_at,
                updated_at,
                deployment_id,
                user_id,
                stripe_subscription_id,
                stripe_customer_id,
                billing_plan_id,
                status as "status!: crate::models::SubscriptionStatus",
                current_period_start,
                current_period_end,
                trial_start,
                trial_end,
                cancel_at_period_end,
                canceled_at,
                ended_at,
                collection_method as "collection_method!: crate::models::CollectionMethod",
                customer_email,
                customer_name,
                metadata
            FROM deployment_subscriptions 
            WHERE stripe_subscription_id = $1
            "#,
            self.stripe_subscription_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Subscription not found".to_string()),
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(subscription)
    }
}