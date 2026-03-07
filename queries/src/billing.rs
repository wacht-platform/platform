use chrono::{DateTime, Utc};
use common::error::AppError;
use models::billing::{BillingAccount, BillingAccountWithSubscription, Subscription};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;

pub struct GetBillingAccountQuery {
    owner_id: String,
}

impl GetBillingAccountQuery {
    pub fn new(owner_id: String) -> Self {
        Self { owner_id }
    }

    pub fn for_user(user_id: i64) -> Self {
        Self {
            owner_id: format!("user_{}", user_id),
        }
    }

    pub fn for_organization(org_id: i64) -> Self {
        Self {
            owner_id: format!("org_{}", org_id),
        }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Option<BillingAccountWithSubscription>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let billing_account = sqlx::query_as::<_, BillingAccount>(
            r#"
            SELECT
                id, owner_id, owner_type, provider_customer_id, legal_name, tax_id, billing_email, billing_phone,
                address_line1, address_line2, city, state, postal_code, country, status,
                payment_method_status, currency, locale, pulse_balance_cents,
                COALESCE(pulse_usage_disabled, false) AS pulse_usage_disabled,
                COALESCE(pulse_notified_below_five, false) AS pulse_notified_below_five,
                COALESCE(pulse_notified_below_zero, false) AS pulse_notified_below_zero,
                COALESCE(pulse_notified_disabled, false) AS pulse_notified_disabled,
                last_checkout_session_id,
                checkout_flow_state,
                last_payment_succeeded_at,
                last_subscription_activated_at,
                last_billing_webhook_event,
                checkout_flow_error,
                last_checkout_session_created_at,
                created_at, updated_at
            FROM billing_accounts WHERE owner_id = $1
            "#
        )
        .bind(&self.owner_id)
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(account) = billing_account {
            let subscription = sqlx::query_as::<_, Subscription>(
                r#"
                SELECT
                    s.id, s.billing_account_id, s.provider_customer_id, s.provider_subscription_id,
                    s.product_id, dp.plan_name, s.status, s.previous_billing_date, s.created_at, s.updated_at
                FROM subscriptions s
                LEFT JOIN dodo_products dp ON s.product_id = dp.product_id
                WHERE s.billing_account_id = $1
                "#
            )
            .bind(account.id)
            .fetch_optional(&mut *conn)
            .await?;

            Ok(Some(BillingAccountWithSubscription {
                billing_account: account,
                subscription,
            }))
        } else {
            Ok(None)
        }
    }
}

pub struct GetSubscriptionByProviderIdQuery {
    provider_subscription_id: String,
}

impl GetSubscriptionByProviderIdQuery {
    pub fn new(provider_subscription_id: String) -> Self {
        Self {
            provider_subscription_id,
        }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Option<BillingAccountWithSubscription>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let subscription = sqlx::query_as::<_, Subscription>(
            r#"
            SELECT
                s.id, s.billing_account_id, s.provider_customer_id, s.provider_subscription_id,
                s.product_id, dp.plan_name, s.status, s.previous_billing_date, s.created_at, s.updated_at
            FROM subscriptions s
            LEFT JOIN dodo_products dp ON s.product_id = dp.product_id
            WHERE s.provider_subscription_id = $1
            "#
        )
        .bind(&self.provider_subscription_id)
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(sub) = subscription {
            let billing_account = sqlx::query_as::<_, BillingAccount>(
                r#"
                SELECT
                    id, owner_id, owner_type, provider_customer_id, legal_name, tax_id, billing_email, billing_phone,
                    address_line1, address_line2, city, state, postal_code, country, status,
                    payment_method_status, currency, locale, pulse_balance_cents,
                    COALESCE(pulse_usage_disabled, false) AS pulse_usage_disabled,
                    COALESCE(pulse_notified_below_five, false) AS pulse_notified_below_five,
                    COALESCE(pulse_notified_below_zero, false) AS pulse_notified_below_zero,
                    COALESCE(pulse_notified_disabled, false) AS pulse_notified_disabled,
                    last_checkout_session_id,
                    checkout_flow_state,
                    last_payment_succeeded_at,
                    last_subscription_activated_at,
                    last_billing_webhook_event,
                    checkout_flow_error,
                    last_checkout_session_created_at,
                    created_at, updated_at
                FROM billing_accounts WHERE id = $1
                "#
            )
            .bind(sub.billing_account_id)
            .fetch_one(&mut *conn)
            .await?;

            Ok(Some(BillingAccountWithSubscription {
                billing_account,
                subscription: Some(sub),
            }))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderSubscriptionInfo {
    pub provider_customer_id: String,
    pub provider_subscription_id: String,
    pub billing_account_id: i64,
    pub owner_id: String,
    pub plan_name: String,
    pub previous_billing_date: Option<DateTime<Utc>>,
}

pub struct GetDeploymentProviderSubscriptionQuery {
    deployment_id: i64,
}

impl GetDeploymentProviderSubscriptionQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Option<ProviderSubscriptionInfo>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query!(
            r#"
            SELECT s.provider_customer_id, s.provider_subscription_id, s.billing_account_id, ba.owner_id, dp.plan_name, s.previous_billing_date
            FROM deployments d
            JOIN projects p ON d.project_id = p.id
            JOIN billing_accounts ba ON p.billing_account_id = ba.id
            JOIN subscriptions s ON s.billing_account_id = ba.id
            LEFT JOIN dodo_products dp ON s.product_id = dp.product_id
            WHERE d.id = $1 AND s.status = 'active'
            LIMIT 1
            "#,
            self.deployment_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(row.map(|r| ProviderSubscriptionInfo {
            provider_customer_id: r.provider_customer_id,
            provider_subscription_id: r.provider_subscription_id,
            billing_account_id: r.billing_account_id,
            owner_id: r.owner_id,
            plan_name: r.plan_name,
            previous_billing_date: r.previous_billing_date,
        }))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UsageSnapshot {
    pub metric_name: String,
    pub quantity: i64,
    pub cost_cents: Option<Decimal>,
}

pub struct GetDeploymentUsageQuery {
    pub deployment_id: i64,
    pub billing_period: DateTime<Utc>,
}

impl GetDeploymentUsageQuery {
    pub fn new(deployment_id: i64, billing_period: DateTime<Utc>) -> Self {
        Self {
            deployment_id,
            billing_period,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<UsageSnapshot>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query!(
            r#"
            SELECT metric_name, quantity, cost_cents
            FROM billing_usage_snapshots
            WHERE deployment_id = $1 AND billing_period = $2
            ORDER BY metric_name
            "#,
            self.deployment_id,
            self.billing_period
        )
        .fetch_all(&mut *conn)
        .await?;

        let snapshots = rows
            .into_iter()
            .map(|r| UsageSnapshot {
                metric_name: r.metric_name,
                quantity: r.quantity,
                cost_cents: r.cost_cents,
            })
            .collect();

        Ok(snapshots)
    }
}

pub struct GetBillingAccountUsageQuery {
    pub billing_account_id: i64,
    pub billing_period: DateTime<Utc>,
}

impl GetBillingAccountUsageQuery {
    pub fn new(billing_account_id: i64, billing_period: DateTime<Utc>) -> Self {
        Self {
            billing_account_id,
            billing_period,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<UsageSnapshot>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query!(
            r#"
            SELECT metric_name, SUM(quantity) as quantity, SUM(cost_cents) as cost_cents
            FROM billing_usage_snapshots
            WHERE billing_account_id = $1 AND billing_period = $2
            GROUP BY metric_name
            ORDER BY metric_name
            "#,
            self.billing_account_id,
            self.billing_period
        )
        .fetch_all(&mut *conn)
        .await?;

        let snapshots = rows
            .into_iter()
            .map(|r| UsageSnapshot {
                metric_name: r.metric_name,
                quantity: r.quantity.unwrap_or(Decimal::ZERO).try_into().unwrap_or(0),
                cost_cents: r.cost_cents,
            })
            .collect();

        Ok(snapshots)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DodoProduct {
    pub id: i32,
    pub plan_name: String,
    pub product_id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub base_price_cents: i32,
    pub is_active: bool,
}

pub struct GetDodoProductQuery {
    plan_name: String,
}

impl GetDodoProductQuery {
    pub fn new(plan_name: impl Into<String>) -> Self {
        Self {
            plan_name: plan_name.into(),
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Option<DodoProduct>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query!(
            r#"
            SELECT id, plan_name, product_id, display_name, description, base_price_cents, is_active
            FROM dodo_products
            WHERE plan_name = $1 AND is_active = true
            "#,
            &self.plan_name
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(row.map(|r| DodoProduct {
            id: r.id,
            plan_name: r.plan_name,
            product_id: r.product_id,
            display_name: r.display_name,
            description: r.description,
            base_price_cents: r.base_price_cents,
            is_active: r.is_active,
        }))
    }
}

pub struct GetBillingAccountByProviderCustomerIdQuery {
    provider_customer_id: String,
}

impl GetBillingAccountByProviderCustomerIdQuery {
    pub fn new(provider_customer_id: impl Into<String>) -> Self {
        Self {
            provider_customer_id: provider_customer_id.into(),
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Option<String>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let owner_id = sqlx::query_scalar!(
            "SELECT owner_id FROM billing_accounts WHERE provider_customer_id = $1",
            &self.provider_customer_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(owner_id)
    }
}

pub struct GetAllDodoProductsQuery;

impl GetAllDodoProductsQuery {
    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<DodoProduct>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query!(
            r#"
            SELECT id, plan_name, product_id, display_name, description, base_price_cents, is_active
            FROM dodo_products
            WHERE is_active = true
            ORDER BY base_price_cents ASC
            "#
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| DodoProduct {
                id: r.id,
                plan_name: r.plan_name,
                product_id: r.product_id,
                display_name: r.display_name,
                description: r.description,
                base_price_cents: r.base_price_cents,
                is_active: r.is_active,
            })
            .collect())
    }
}

pub struct GetOwnerIdByDeploymentIdQuery {
    pub deployment_id: i64,
}

impl GetOwnerIdByDeploymentIdQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Option<String>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query(
            r#"
            SELECT p.owner_id
            FROM deployments d
            JOIN projects p ON d.project_id = p.id
            WHERE d.id = $1
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(&mut *conn)
        .await?;

        use sqlx::Row;
        Ok(row.and_then(|r| r.get("owner_id")))
    }
}

pub struct ListPulseTransactionsQuery {
    pub billing_account_id: i64,
}

impl ListPulseTransactionsQuery {
    pub fn new(billing_account_id: i64) -> Self {
        Self { billing_account_id }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Vec<models::pulse_transaction::PulseTransaction>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query_as::<_, models::pulse_transaction::PulseTransaction>(
            r#"
            SELECT id, billing_account_id, amount_pulse_cents, transaction_type, reference_id, created_at
            FROM pulse_transactions
            WHERE billing_account_id = $1
            ORDER BY created_at DESC
            "#
        )
        .bind(self.billing_account_id)
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows)
    }
}

pub struct ListBillingInvoicesQuery {
    pub billing_account_id: i64,
}

impl ListBillingInvoicesQuery {
    pub fn new(billing_account_id: i64) -> Self {
        Self { billing_account_id }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Vec<models::billing_invoice::BillingInvoice>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query(
            r#"
            SELECT
                id, created_at, updated_at, billing_account_id, subscription_id,
                provider_payment_id, provider_customer_id, amount_due_cents,
                amount_paid_cents, currency, status,
                invoice_pdf_url, hosted_invoice_url, invoice_number, due_date,
                paid_at, period_start, period_end, attempt_count, next_payment_attempt,
                metadata
            FROM billing_invoices
            WHERE billing_account_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(self.billing_account_id)
        .fetch_all(&mut *conn)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let invoice = models::billing_invoice::BillingInvoice {
                id: row.try_get("id")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
                billing_account_id: row.try_get("billing_account_id")?,
                subscription_id: row.try_get("subscription_id")?,
                provider_payment_id: row.try_get("provider_payment_id")?,
                provider_customer_id: row.try_get("provider_customer_id")?,
                amount_due_cents: row.try_get("amount_due_cents")?,
                amount_paid_cents: row.try_get("amount_paid_cents")?,
                currency: row.try_get("currency")?,
                status: row.try_get("status")?,
                invoice_pdf_url: row.try_get("invoice_pdf_url")?,
                hosted_invoice_url: row.try_get("hosted_invoice_url")?,
                invoice_number: row.try_get("invoice_number")?,
                due_date: row.try_get("due_date")?,
                paid_at: row.try_get("paid_at")?,
                period_start: row.try_get("period_start")?,
                period_end: row.try_get("period_end")?,
                attempt_count: row.try_get("attempt_count")?,
                next_payment_attempt: row.try_get("next_payment_attempt")?,
                metadata: row.try_get("metadata")?,
            };
            results.push(invoice);
        }

        Ok(results)
    }
}
