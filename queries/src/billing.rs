use models::billing::Subscription;
use crate::{Query};
use common::error::AppError;
use common::state::AppState;

// Get subscription for a user
pub struct GetUserSubscriptionQuery {
    user_id: i64,
}

impl GetUserSubscriptionQuery {
    pub fn new(user_id: i64) -> Self {
        Self { user_id }
    }
}

impl Query for GetUserSubscriptionQuery {
    type Output = Option<Subscription>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            SELECT * FROM subscriptions
            WHERE user_id = $1
            "#,
            self.user_id
        )
        .fetch_optional(&state.db_pool)
        .await?;
        
        Ok(subscription)
    }
}

// Get subscription for an organization
pub struct GetOrganizationSubscriptionQuery {
    organization_id: i64,
}

impl GetOrganizationSubscriptionQuery {
    pub fn new(organization_id: i64) -> Self {
        Self { organization_id }
    }
}

impl Query for GetOrganizationSubscriptionQuery {
    type Output = Option<Subscription>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            SELECT * FROM subscriptions
            WHERE organization_id = $1
            "#,
            self.organization_id
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