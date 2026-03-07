use chrono::{DateTime, Utc};
use common::error::AppError;
use models::billing::Subscription;

const SUBSCRIPTION_RETURNING_COLUMNS: &str = r#"
id,
billing_account_id,
provider_customer_id,
provider_subscription_id,
product_id,
status,
previous_billing_date,
created_at,
updated_at
"#;

pub struct CreateSubscriptionCommand {
    id: i64,
    billing_account_id: i64,
    provider_customer_id: String,
    provider_subscription_id: String,
    status: String,
}

impl CreateSubscriptionCommand {
    pub fn new(
        id: i64,
        billing_account_id: i64,
        provider_customer_id: String,
        provider_subscription_id: String,
        status: String,
    ) -> Self {
        Self {
            id,
            billing_account_id,
            provider_customer_id,
            provider_subscription_id,
            status,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<Subscription, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let sql = format!(
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
            RETURNING {returning}
            "#,
            returning = SUBSCRIPTION_RETURNING_COLUMNS
        );
        let subscription = sqlx::query_as::<_, Subscription>(&sql)
            .bind(self.id)
            .bind(self.billing_account_id)
            .bind(&self.provider_customer_id)
            .bind(&self.provider_subscription_id)
            .bind(&self.status)
            .fetch_one(executor)
            .await?;

        Ok(subscription)
    }
}

pub struct UpdateSubscriptionStatusCommand {
    pub subscription_id: i64,
    pub status: String,
}

impl UpdateSubscriptionStatusCommand {
    pub fn new(subscription_id: i64, status: String) -> Self {
        Self {
            subscription_id,
            status,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<Subscription, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let sql = format!(
            r#"
            UPDATE subscriptions 
            SET status = $1, updated_at = NOW()
            WHERE id = $2
            RETURNING {returning}
            "#,
            returning = SUBSCRIPTION_RETURNING_COLUMNS
        );
        let subscription = sqlx::query_as::<_, Subscription>(&sql)
            .bind(&self.status)
            .bind(self.subscription_id)
            .fetch_one(executor)
            .await?;

        Ok(subscription)
    }
}

pub struct UpsertSubscriptionCommand {
    id: i64,
    owner_id: String,
    provider_customer_id: String,
    provider_subscription_id: String,
    product_id: Option<String>,
    status: String,
    previous_billing_date: Option<DateTime<Utc>>,
}

impl UpsertSubscriptionCommand {
    pub fn new(
        id: i64,
        owner_id: String,
        provider_customer_id: String,
        provider_subscription_id: String,
        status: String,
    ) -> Self {
        Self {
            id,
            owner_id,
            provider_customer_id,
            provider_subscription_id,
            product_id: None,
            status,
            previous_billing_date: None,
        }
    }

    pub fn with_product_id(mut self, product_id: Option<String>) -> Self {
        self.product_id = product_id;
        self
    }

    pub fn with_previous_billing_date(
        mut self,
        previous_billing_date: Option<DateTime<Utc>>,
    ) -> Self {
        self.previous_billing_date = previous_billing_date;
        self
    }

    pub async fn execute_with_db<'a, A>(self, executor: A) -> Result<Subscription, AppError>
    where
        A: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            WITH account AS (
                SELECT id AS billing_account_id
                FROM billing_accounts
                WHERE owner_id = $1
            ),
            existing AS (
                SELECT s.id
                FROM subscriptions s
                WHERE s.billing_account_id = (SELECT billing_account_id FROM account)
            ),
            upsert AS (
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
                )
                SELECT
                    COALESCE((SELECT id FROM existing), $2),
                    (SELECT billing_account_id FROM account),
                    $3,
                    $4,
                    $5,
                    $6,
                    $7,
                    NOW(),
                    NOW()
                WHERE EXISTS(SELECT 1 FROM account)
                ON CONFLICT (id) DO UPDATE SET
                    provider_customer_id = EXCLUDED.provider_customer_id,
                    provider_subscription_id = EXCLUDED.provider_subscription_id,
                    product_id = EXCLUDED.product_id,
                    status = EXCLUDED.status,
                    previous_billing_date = EXCLUDED.previous_billing_date,
                    updated_at = NOW()
                RETURNING id, billing_account_id, provider_customer_id, provider_subscription_id, product_id, status, previous_billing_date, created_at, updated_at
            )
            SELECT
                EXISTS(SELECT 1 FROM account) AS "account_exists!",
                upsert.id,
                upsert.billing_account_id,
                upsert.provider_customer_id,
                upsert.provider_subscription_id,
                upsert.product_id,
                upsert.status,
                upsert.previous_billing_date,
                upsert.created_at,
                upsert.updated_at
            FROM upsert
            UNION ALL
            SELECT
                EXISTS(SELECT 1 FROM account) AS "account_exists!",
                NULL::BIGINT AS id,
                NULL::BIGINT AS billing_account_id,
                NULL::TEXT AS provider_customer_id,
                NULL::TEXT AS provider_subscription_id,
                NULL::TEXT AS product_id,
                NULL::TEXT AS status,
                NULL::TIMESTAMPTZ AS previous_billing_date,
                NOW() AS created_at,
                NOW() AS updated_at
            WHERE NOT EXISTS(SELECT 1 FROM upsert)
            LIMIT 1
            "#,
            self.owner_id,
            self.id,
            self.provider_customer_id,
            self.provider_subscription_id,
            self.product_id,
            self.status,
            self.previous_billing_date
        )
        .fetch_one(executor)
        .await?;

        if !row.account_exists {
            return Err(AppError::Validation(
                "Billing account not found for owner".to_string(),
            ));
        }

        Ok(Subscription {
            id: row.id.unwrap_or(self.id),
            billing_account_id: row.billing_account_id.unwrap_or_default(),
            provider_customer_id: row.provider_customer_id.unwrap_or_default(),
            provider_subscription_id: row.provider_subscription_id.unwrap_or_default(),
            product_id: row.product_id,
            plan_name: None,
            status: row.status.unwrap_or_default(),
            previous_billing_date: row.previous_billing_date,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}
