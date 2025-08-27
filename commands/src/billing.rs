use models::billing::Subscription;
use crate::Command;
use common::error::AppError;
use common::state::AppState;

// Create or update subscription
pub struct UpsertSubscriptionCommand {
    pub user_id: Option<i64>,
    pub organization_id: Option<i64>,
    pub chargebee_customer_id: String,
    pub chargebee_subscription_id: String,
    pub status: String,
}

impl Command for UpsertSubscriptionCommand {
    type Output = Subscription;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        // Validate that either user_id or organization_id is provided, but not both
        match (self.user_id, self.organization_id) {
            (Some(_), Some(_)) => {
                return Err(AppError::Validation(
                    "Subscription cannot belong to both user and organization".to_string()
                ));
            }
            (None, None) => {
                return Err(AppError::Validation(
                    "Subscription must belong to either user or organization".to_string()
                ));
            }
            _ => {}
        }
        
        // Check if subscription already exists for user or organization
        let existing_id: Option<i64> = if let Some(user_id) = self.user_id {
            sqlx::query_scalar!("SELECT id FROM subscriptions WHERE user_id = $1", user_id)
                .fetch_optional(&state.db_pool)
                .await?
        } else if let Some(org_id) = self.organization_id {
            sqlx::query_scalar!("SELECT id FROM subscriptions WHERE organization_id = $1", org_id)
                .fetch_optional(&state.db_pool)
                .await?
        } else {
            None
        };
        
        let subscription = if let Some(id) = existing_id {
            // Update existing subscription
            sqlx::query_as!(
                Subscription,
                r#"
                UPDATE subscriptions SET
                    chargebee_customer_id = $1,
                    chargebee_subscription_id = $2,
                    status = $3,
                    updated_at = NOW()
                WHERE id = $4
                RETURNING *
                "#,
                self.chargebee_customer_id,
                self.chargebee_subscription_id,
                self.status,
                id
            )
            .fetch_one(&state.db_pool)
            .await?
        } else {
            // Create new subscription
            let id = state.sf.next_id().unwrap() as i64;
            sqlx::query_as!(
                Subscription,
                r#"
                INSERT INTO subscriptions (
                    id,
                    user_id,
                    organization_id,
                    chargebee_customer_id,
                    chargebee_subscription_id,
                    status,
                    created_at,
                    updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
                RETURNING *
                "#,
                id,
                self.user_id,
                self.organization_id,
                self.chargebee_customer_id,
                self.chargebee_subscription_id,
                self.status
            )
            .fetch_one(&state.db_pool)
            .await?
        };
        
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