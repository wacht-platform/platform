use models::billing::Subscription;
use crate::Command;
use common::error::AppError;
use common::state::AppState;

// Create or update subscription
pub struct UpsertSubscriptionCommand {
    pub project_id: i64,
    pub chargebee_customer_id: String,
    pub chargebee_subscription_id: String,
    pub status: String,
}

impl Command for UpsertSubscriptionCommand {
    type Output = Subscription;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let id = state.sf.next_id().unwrap() as i64;
        
        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            INSERT INTO subscriptions (
                id,
                project_id, 
                chargebee_customer_id, 
                chargebee_subscription_id,
                status,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
            ON CONFLICT (project_id) DO UPDATE SET
                chargebee_customer_id = EXCLUDED.chargebee_customer_id,
                chargebee_subscription_id = EXCLUDED.chargebee_subscription_id,
                status = EXCLUDED.status,
                updated_at = NOW()
            RETURNING *
            "#,
            id,
            self.project_id,
            self.chargebee_customer_id,
            self.chargebee_subscription_id,
            self.status
        )
        .fetch_one(&state.db_pool)
        .await?;
        
        Ok(subscription)
    }
}

// Update subscription status
pub struct UpdateSubscriptionStatusCommand {
    pub subscription_id: i64,
    pub status: String,
}

impl Command for UpdateSubscriptionStatusCommand {
    type Output = Subscription;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            UPDATE subscriptions 
            SET status = $1, updated_at = NOW()
            WHERE id = $2
            RETURNING *
            "#,
            self.status,
            self.subscription_id
        )
        .fetch_one(&state.db_pool)
        .await?;
        
        Ok(subscription)
    }
}