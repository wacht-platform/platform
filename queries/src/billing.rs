use models::billing::Subscription;
use crate::{Query};
use common::error::AppError;
use common::state::AppState;

// Get subscription for a project
pub struct GetProjectSubscriptionQuery {
    project_id: i64,
}

impl GetProjectSubscriptionQuery {
    pub fn new(project_id: i64) -> Self {
        Self { project_id }
    }
}

impl Query for GetProjectSubscriptionQuery {
    type Output = Option<Subscription>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            SELECT * FROM subscriptions
            WHERE project_id = $1
            "#,
            self.project_id
        )
        .fetch_optional(&state.db_pool)
        .await?;
        
        Ok(subscription)
    }
}

// Get subscription by Chargebee ID
pub struct GetSubscriptionByChargebeeIdQuery {
    chargebee_subscription_id: String,
}

impl GetSubscriptionByChargebeeIdQuery {
    pub fn new(chargebee_subscription_id: String) -> Self {
        Self { chargebee_subscription_id }
    }
}

impl Query for GetSubscriptionByChargebeeIdQuery {
    type Output = Option<Subscription>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            SELECT * FROM subscriptions
            WHERE chargebee_subscription_id = $1
            "#,
            self.chargebee_subscription_id
        )
        .fetch_optional(&state.db_pool)
        .await?;
        
        Ok(subscription)
    }
}