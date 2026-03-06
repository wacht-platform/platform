use chrono::{DateTime, Utc};
use common::error::AppError;
use models::billing::Subscription;

pub struct CreateSubscriptionCommand {
    pub billing_account_id: i64,
    pub provider_customer_id: String,
    pub provider_subscription_id: String,
    pub status: String,
}

impl CreateSubscriptionCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        subscription_id: i64,
    ) -> Result<Subscription, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let subscription = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (
                id,
                billing_account_id,
                provider_customer_id,
                provider_subscription_id,
                status,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
            RETURNING id, billing_account_id, provider_customer_id, provider_subscription_id, product_id, status, previous_billing_date, created_at, updated_at
            "#
        )
        .bind(subscription_id)
        .bind(self.billing_account_id)
        .bind(&self.provider_customer_id)
        .bind(&self.provider_subscription_id)
        .bind(&self.status)
        .fetch_one(&mut *conn)
        .await?;

        Ok(subscription)
    }
}

pub struct UpdateSubscriptionStatusCommand {
    pub subscription_id: i64,
    pub status: String,
}

impl UpdateSubscriptionStatusCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<Subscription, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let subscription = sqlx::query_as::<_, Subscription>(
            r#"
            UPDATE subscriptions 
            SET status = $1, updated_at = NOW()
            WHERE id = $2
            RETURNING id, billing_account_id, provider_customer_id, provider_subscription_id, product_id, status, previous_billing_date, created_at, updated_at
            "#
        )
        .bind(&self.status)
        .bind(self.subscription_id)
        .fetch_one(&mut *conn)
        .await?;

        Ok(subscription)
    }
}

pub struct UpsertSubscriptionCommand {
    pub owner_id: String,
    pub provider_customer_id: String,
    pub provider_subscription_id: String,
    pub product_id: Option<String>,
    pub status: String,
    pub previous_billing_date: Option<DateTime<Utc>>,
}

impl UpsertSubscriptionCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        subscription_id: i64,
    ) -> Result<Subscription, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let billing_account_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM billing_accounts WHERE owner_id = $1",
            self.owner_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        let billing_account_id = match billing_account_id {
            Some(id) => id,
            None => {
                return Err(AppError::Validation(
                    "Billing account not found for owner".to_string(),
                ));
            }
        };

        let existing_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM subscriptions WHERE billing_account_id = $1",
            billing_account_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        let subscription = if let Some(id) = existing_id {
            sqlx::query_as::<_, Subscription>(
                r#"
                UPDATE subscriptions SET
                    provider_customer_id = $1,
                    provider_subscription_id = $2,
                    product_id = $3,
                    status = $4,
                    previous_billing_date = $5,
                    updated_at = NOW()
                WHERE id = $6
                RETURNING id, billing_account_id, provider_customer_id, provider_subscription_id, product_id, status, previous_billing_date, created_at, updated_at
                "#,
            )
            .bind(&self.provider_customer_id)
            .bind(&self.provider_subscription_id)
            .bind(&self.product_id)
            .bind(&self.status)
            .bind(self.previous_billing_date)
            .bind(id)
            .fetch_one(&mut *conn)
            .await?
        } else {
            sqlx::query_as::<_, Subscription>(
                r#"
                INSERT INTO subscriptions (
                    id,
                    billing_account_id,
                    provider_customer_id,
                    provider_subscription_id,
                    product_id,
                    status,
                    previous_billing_date,
                    created_at,
                    updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
                RETURNING id, billing_account_id, provider_customer_id, provider_subscription_id, product_id, status, previous_billing_date, created_at, updated_at
                "#,
            )
            .bind(subscription_id)
            .bind(billing_account_id)
            .bind(&self.provider_customer_id)
            .bind(&self.provider_subscription_id)
            .bind(&self.product_id)
            .bind(&self.status)
            .bind(self.previous_billing_date)
            .fetch_one(&mut *conn)
            .await?
        };

        Ok(subscription)
    }
}
