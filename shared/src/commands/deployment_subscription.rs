use crate::{
    error::AppError,
    models::{DeploymentSubscription, SubscriptionStatus, CollectionMethod},
    state::AppState,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::Command;

pub struct CreateDeploymentSubscriptionCommand {
    pub deployment_id: i64,
    pub billing_plan_id: i64,
    pub user_id: Option<i64>,
    pub stripe_subscription_id: String,
    pub stripe_customer_id: String,
    pub status: SubscriptionStatus,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub trial_start: Option<DateTime<Utc>>,
    pub trial_end: Option<DateTime<Utc>>,
    pub collection_method: CollectionMethod,
    pub customer_email: String,
    pub customer_name: Option<String>,
    pub metadata: Option<Value>,
}

impl Command for CreateDeploymentSubscriptionCommand {
    type Output = DeploymentSubscription;

    async fn execute(self, app_state: &AppState) -> Result<DeploymentSubscription, AppError> {
        let subscription = sqlx::query_as!(
            DeploymentSubscription,
            r#"
            INSERT INTO deployment_subscriptions (
                deployment_id,
                billing_plan_id,
                user_id,
                stripe_subscription_id,
                stripe_customer_id,
                status,
                current_period_start,
                current_period_end,
                trial_start,
                trial_end,
                collection_method,
                customer_email,
                customer_name,
                metadata
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            RETURNING 
                id,
                created_at,
                updated_at,
                deployment_id,
                billing_plan_id,
                user_id,
                stripe_subscription_id,
                stripe_customer_id,
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
            "#,
            self.deployment_id,
            self.billing_plan_id,
            self.user_id,
            self.stripe_subscription_id,
            self.stripe_customer_id,
            self.status as SubscriptionStatus,
            self.current_period_start,
            self.current_period_end,
            self.trial_start,
            self.trial_end,
            self.collection_method as CollectionMethod,
            self.customer_email,
            self.customer_name,
            self.metadata
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.constraint() == Some("deployment_subscriptions_deployment_id_fkey") => {
                AppError::BadRequest("Invalid deployment ID".to_string())
            }
            sqlx::Error::Database(db_err) if db_err.constraint() == Some("deployment_subscriptions_billing_plan_id_fkey") => {
                AppError::BadRequest("Invalid billing plan ID".to_string())
            }
            sqlx::Error::Database(db_err) if db_err.constraint() == Some("deployment_subscriptions_user_id_fkey") => {
                AppError::BadRequest("Invalid user ID".to_string())
            }
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(subscription)
    }
}

pub struct UpdateDeploymentSubscriptionCommand {
    pub deployment_id: i64,
    pub subscription_id: i64,
    pub status: Option<SubscriptionStatus>,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub trial_start: Option<Option<DateTime<Utc>>>,
    pub trial_end: Option<Option<DateTime<Utc>>>,
    pub cancel_at_period_end: Option<bool>,
    pub canceled_at: Option<Option<DateTime<Utc>>>,
    pub ended_at: Option<Option<DateTime<Utc>>>,
    pub collection_method: Option<CollectionMethod>,
    pub customer_email: Option<String>,
    pub customer_name: Option<Option<String>>,
    pub metadata: Option<Option<Value>>,
}

impl Command for UpdateDeploymentSubscriptionCommand {
    type Output = DeploymentSubscription;

    async fn execute(self, app_state: &AppState) -> Result<DeploymentSubscription, AppError> {
        let subscription = sqlx::query_as!(
            DeploymentSubscription,
            r#"
            UPDATE deployment_subscriptions 
            SET 
                updated_at = NOW(),
                status = COALESCE($3, status),
                current_period_start = COALESCE($4, current_period_start),
                current_period_end = COALESCE($5, current_period_end),
                trial_start = COALESCE($6, trial_start),
                trial_end = COALESCE($7, trial_end),
                cancel_at_period_end = COALESCE($8, cancel_at_period_end),
                canceled_at = COALESCE($9, canceled_at),
                ended_at = COALESCE($10, ended_at),
                collection_method = COALESCE($11, collection_method),
                customer_email = COALESCE($12, customer_email),
                customer_name = COALESCE($13, customer_name),
                metadata = COALESCE($14, metadata)
            WHERE deployment_id = $1 AND id = $2
            RETURNING 
                id,
                created_at,
                updated_at,
                deployment_id,
                billing_plan_id,
                user_id,
                stripe_subscription_id,
                stripe_customer_id,
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
            "#,
            self.deployment_id,
            self.subscription_id,
            self.status as Option<SubscriptionStatus>,
            self.current_period_start,
            self.current_period_end,
            self.trial_start.flatten(),
            self.trial_end.flatten(),
            self.cancel_at_period_end,
            self.canceled_at.flatten(),
            self.ended_at.flatten(),
            self.collection_method as Option<CollectionMethod>,
            self.customer_email,
            self.customer_name.flatten(),
            self.metadata.flatten()
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

pub struct CancelDeploymentSubscriptionCommand {
    pub deployment_id: i64,
    pub subscription_id: i64,
    pub cancel_at_period_end: bool,
}

impl Command for CancelDeploymentSubscriptionCommand {
    type Output = DeploymentSubscription;

    async fn execute(self, app_state: &AppState) -> Result<DeploymentSubscription, AppError> {
        let now = Utc::now();
        
        let subscription = if self.cancel_at_period_end {
            // Cancel at period end
            sqlx::query_as!(
                DeploymentSubscription,
                r#"
                UPDATE deployment_subscriptions 
                SET 
                    updated_at = NOW(),
                    cancel_at_period_end = true,
                    canceled_at = $3
                WHERE deployment_id = $1 AND id = $2
                RETURNING 
                    id,
                    created_at,
                    updated_at,
                    deployment_id,
                    billing_plan_id,
                    user_id,
                    stripe_subscription_id,
                    stripe_customer_id,
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
                "#,
                self.deployment_id,
                self.subscription_id,
                now
            )
            .fetch_one(&app_state.db_pool)
            .await
        } else {
            // Cancel immediately
            sqlx::query_as!(
                DeploymentSubscription,
                r#"
                UPDATE deployment_subscriptions 
                SET 
                    updated_at = NOW(),
                    status = 'canceled',
                    canceled_at = $3,
                    ended_at = $3
                WHERE deployment_id = $1 AND id = $2
                RETURNING 
                    id,
                    created_at,
                    updated_at,
                    deployment_id,
                    billing_plan_id,
                    user_id,
                    stripe_subscription_id,
                    stripe_customer_id,
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
                "#,
                self.deployment_id,
                self.subscription_id,
                now
            )
            .fetch_one(&app_state.db_pool)
            .await
        }
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Subscription not found".to_string()),
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(subscription)
    }
}