use chrono::{DateTime, Utc};
use common::error::AppError;
use models::billing::{BillingAccount, BillingAccountWithSubscription, Subscription};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;

const BILLING_ACCOUNT_WITH_SUBSCRIPTION_SELECT: &str = r#"
SELECT
    ba.id AS ba_id, ba.owner_id AS ba_owner_id, ba.owner_type AS ba_owner_type,
    ba.provider_customer_id AS ba_provider_customer_id, ba.legal_name AS ba_legal_name,
    ba.tax_id AS ba_tax_id, ba.billing_email AS ba_billing_email, ba.billing_phone AS ba_billing_phone,
    ba.address_line1 AS ba_address_line1, ba.address_line2 AS ba_address_line2,
    ba.city AS ba_city, ba.state AS ba_state, ba.postal_code AS ba_postal_code,
    ba.country AS ba_country, ba.status AS ba_status,
    ba.payment_method_status AS ba_payment_method_status, ba.currency AS ba_currency,
    ba.locale AS ba_locale, ba.pulse_balance_cents AS ba_pulse_balance_cents,
    ba.max_projects_per_account AS ba_max_projects_per_account,
    ba.max_staging_deployments_per_project AS ba_max_staging_deployments_per_project,
    COALESCE(ba.pulse_usage_disabled, false) AS ba_pulse_usage_disabled,
    COALESCE(ba.pulse_notified_below_five, false) AS ba_pulse_notified_below_five,
    COALESCE(ba.pulse_notified_below_zero, false) AS ba_pulse_notified_below_zero,
    COALESCE(ba.pulse_notified_disabled, false) AS ba_pulse_notified_disabled,
    ba.last_checkout_session_id AS ba_last_checkout_session_id,
    ba.checkout_flow_state AS ba_checkout_flow_state,
    ba.last_payment_succeeded_at AS ba_last_payment_succeeded_at,
    ba.last_subscription_activated_at AS ba_last_subscription_activated_at,
    ba.last_billing_webhook_event AS ba_last_billing_webhook_event,
    ba.checkout_flow_error AS ba_checkout_flow_error,
    ba.last_checkout_session_created_at AS ba_last_checkout_session_created_at,
    ba.created_at AS ba_created_at, ba.updated_at AS ba_updated_at,
    s.id AS s_id, s.billing_account_id AS s_billing_account_id,
    s.provider_customer_id AS s_provider_customer_id,
    s.provider_subscription_id AS s_provider_subscription_id,
    s.product_id AS s_product_id, s.plan_name AS s_plan_name, s.status AS s_status,
    s.previous_billing_date AS s_previous_billing_date,
    s.created_at AS s_created_at, s.updated_at AS s_updated_at
"#;

fn map_billing_account_with_prefix(row: &sqlx::postgres::PgRow, prefix: &str) -> BillingAccount {
    BillingAccount {
        id: row.get(format!("{prefix}id").as_str()),
        owner_id: row.get(format!("{prefix}owner_id").as_str()),
        owner_type: row.get(format!("{prefix}owner_type").as_str()),
        provider_customer_id: row.get(format!("{prefix}provider_customer_id").as_str()),
        legal_name: row.get(format!("{prefix}legal_name").as_str()),
        tax_id: row.get(format!("{prefix}tax_id").as_str()),
        billing_email: row.get(format!("{prefix}billing_email").as_str()),
        billing_phone: row.get(format!("{prefix}billing_phone").as_str()),
        address_line1: row.get(format!("{prefix}address_line1").as_str()),
        address_line2: row.get(format!("{prefix}address_line2").as_str()),
        city: row.get(format!("{prefix}city").as_str()),
        state: row.get(format!("{prefix}state").as_str()),
        postal_code: row.get(format!("{prefix}postal_code").as_str()),
        country: row.get(format!("{prefix}country").as_str()),
        status: row.get(format!("{prefix}status").as_str()),
        payment_method_status: row.get(format!("{prefix}payment_method_status").as_str()),
        currency: row.get(format!("{prefix}currency").as_str()),
        locale: row.get(format!("{prefix}locale").as_str()),
        pulse_balance_cents: row.get(format!("{prefix}pulse_balance_cents").as_str()),
        max_projects_per_account: row.get(format!("{prefix}max_projects_per_account").as_str()),
        max_staging_deployments_per_project: row
            .get(format!("{prefix}max_staging_deployments_per_project").as_str()),
        pulse_usage_disabled: row.get(format!("{prefix}pulse_usage_disabled").as_str()),
        pulse_notified_below_five: row.get(format!("{prefix}pulse_notified_below_five").as_str()),
        pulse_notified_below_zero: row.get(format!("{prefix}pulse_notified_below_zero").as_str()),
        pulse_notified_disabled: row.get(format!("{prefix}pulse_notified_disabled").as_str()),
        last_checkout_session_id: row.get(format!("{prefix}last_checkout_session_id").as_str()),
        checkout_flow_state: row.get(format!("{prefix}checkout_flow_state").as_str()),
        last_payment_succeeded_at: row.get(format!("{prefix}last_payment_succeeded_at").as_str()),
        last_subscription_activated_at: row
            .get(format!("{prefix}last_subscription_activated_at").as_str()),
        last_billing_webhook_event: row.get(format!("{prefix}last_billing_webhook_event").as_str()),
        checkout_flow_error: row.get(format!("{prefix}checkout_flow_error").as_str()),
        last_checkout_session_created_at: row
            .get(format!("{prefix}last_checkout_session_created_at").as_str()),
        created_at: row.get(format!("{prefix}created_at").as_str()),
        updated_at: row.get(format!("{prefix}updated_at").as_str()),
    }
}

fn map_subscription_with_prefix(row: &sqlx::postgres::PgRow, prefix: &str) -> Option<Subscription> {
    let id: Option<i64> = row.get(format!("{prefix}id").as_str());
    id.map(|id| Subscription {
        id,
        billing_account_id: row.get(format!("{prefix}billing_account_id").as_str()),
        provider_customer_id: row.get(format!("{prefix}provider_customer_id").as_str()),
        provider_subscription_id: row.get(format!("{prefix}provider_subscription_id").as_str()),
        product_id: row.get(format!("{prefix}product_id").as_str()),
        plan_name: row.get(format!("{prefix}plan_name").as_str()),
        status: row.get(format!("{prefix}status").as_str()),
        previous_billing_date: row.get(format!("{prefix}previous_billing_date").as_str()),
        created_at: row.get(format!("{prefix}created_at").as_str()),
        updated_at: row.get(format!("{prefix}updated_at").as_str()),
    })
}

fn map_billing_account_with_subscription(
    row: sqlx::postgres::PgRow,
) -> BillingAccountWithSubscription {
    BillingAccountWithSubscription {
        billing_account: map_billing_account_with_prefix(&row, "ba_"),
        subscription: map_subscription_with_prefix(&row, "s_"),
    }
}

fn map_usage_snapshot_row(row: &sqlx::postgres::PgRow) -> Result<UsageSnapshot, sqlx::Error> {
    Ok(UsageSnapshot {
        metric_name: row.try_get("metric_name")?,
        quantity: row.try_get("quantity")?,
        cost_cents: row.try_get("cost_cents")?,
    })
}

fn map_usage_snapshot_aggregated_row(
    row: &sqlx::postgres::PgRow,
) -> Result<UsageSnapshot, sqlx::Error> {
    let quantity: Option<Decimal> = row.try_get("quantity")?;
    Ok(UsageSnapshot {
        metric_name: row.try_get("metric_name")?,
        quantity: quantity.unwrap_or(Decimal::ZERO).try_into().unwrap_or(0),
        cost_cents: row.try_get("cost_cents")?,
    })
}

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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<BillingAccountWithSubscription>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let sql = format!(
            r#"
            {select}
            FROM billing_accounts ba
            LEFT JOIN LATERAL (
                SELECT
                    s.id, s.billing_account_id, s.provider_customer_id, s.provider_subscription_id,
                    s.product_id, dp.plan_name, s.status, s.previous_billing_date, s.created_at, s.updated_at
                FROM subscriptions s
                LEFT JOIN dodo_products dp ON s.product_id = dp.product_id
                WHERE s.billing_account_id = ba.id
                LIMIT 1
            ) s ON true
            WHERE ba.owner_id = $1
            LIMIT 1
            "#,
            select = BILLING_ACCOUNT_WITH_SUBSCRIPTION_SELECT
        );
        let row = sqlx::query(&sql)
            .bind(&self.owner_id)
            .fetch_optional(executor)
            .await?;

        Ok(row.map(map_billing_account_with_subscription))
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<BillingAccountWithSubscription>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let sql = format!(
            r#"
            {select}
            FROM (
                SELECT
                    s.id,
                    s.billing_account_id,
                    s.provider_customer_id,
                    s.provider_subscription_id,
                    s.product_id,
                    dp.plan_name,
                    s.status,
                    s.previous_billing_date,
                    s.created_at,
                    s.updated_at
                FROM subscriptions s
                LEFT JOIN dodo_products dp ON s.product_id = dp.product_id
            ) s
            JOIN billing_accounts ba ON ba.id = s.billing_account_id
            WHERE s.provider_subscription_id = $1
            LIMIT 1
            "#,
            select = BILLING_ACCOUNT_WITH_SUBSCRIPTION_SELECT
        );
        let row = sqlx::query(&sql)
            .bind(&self.provider_subscription_id)
            .fetch_optional(executor)
            .await?;

        Ok(row.map(map_billing_account_with_subscription))
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProviderSubscriptionInfo>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_optional(executor)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<UsageSnapshot>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query(
            r#"
            SELECT metric_name, quantity, cost_cents
            FROM billing_usage_snapshots
            WHERE deployment_id = $1 AND billing_period = $2
            ORDER BY metric_name
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.billing_period)
        .fetch_all(executor)
        .await?;

        let mut snapshots = Vec::with_capacity(rows.len());
        for row in rows {
            snapshots.push(map_usage_snapshot_row(&row)?);
        }

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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<UsageSnapshot>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query(
            r#"
            SELECT metric_name, SUM(quantity) as quantity, SUM(cost_cents) as cost_cents
            FROM billing_usage_snapshots
            WHERE billing_account_id = $1 AND billing_period = $2
            GROUP BY metric_name
            ORDER BY metric_name
            "#,
        )
        .bind(self.billing_account_id)
        .bind(self.billing_period)
        .fetch_all(executor)
        .await?;

        let mut snapshots = Vec::with_capacity(rows.len());
        for row in rows {
            snapshots.push(map_usage_snapshot_aggregated_row(&row)?);
        }

        Ok(snapshots)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<DodoProduct>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as::<_, DodoProduct>(
            r#"
            SELECT id, plan_name, product_id, display_name, description, base_price_cents, is_active
            FROM dodo_products
            WHERE plan_name = $1 AND is_active = true
            "#,
        )
        .bind(&self.plan_name)
        .fetch_optional(executor)
        .await?;

        Ok(row)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<String>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let owner_id = sqlx::query_scalar!(
            "SELECT owner_id FROM billing_accounts WHERE provider_customer_id = $1",
            &self.provider_customer_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(owner_id)
    }
}

pub struct GetAllDodoProductsQuery;

impl GetAllDodoProductsQuery {
    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<DodoProduct>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as::<_, DodoProduct>(
            r#"
            SELECT id, plan_name, product_id, display_name, description, base_price_cents, is_active
            FROM dodo_products
            WHERE is_active = true
            ORDER BY base_price_cents ASC
            "#,
        )
        .fetch_all(executor)
        .await?;

        Ok(rows)
    }
}

pub struct GetOwnerIdByDeploymentIdQuery {
    pub deployment_id: i64,
}

impl GetOwnerIdByDeploymentIdQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<String>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let owner_id = sqlx::query_scalar(
            r#"
            SELECT p.owner_id
            FROM deployments d
            JOIN projects p ON d.project_id = p.id
            WHERE d.id = $1
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(executor)
        .await?;

        Ok(owner_id)
    }
}

pub struct ListPulseTransactionsQuery {
    pub billing_account_id: i64,
}

impl ListPulseTransactionsQuery {
    pub fn new(billing_account_id: i64) -> Self {
        Self { billing_account_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<models::pulse_transaction::PulseTransaction>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as::<_, models::pulse_transaction::PulseTransaction>(
            r#"
            SELECT id, billing_account_id, amount_pulse_cents, transaction_type, reference_id, created_at
            FROM pulse_transactions
            WHERE billing_account_id = $1
            ORDER BY created_at DESC
            "#
        )
        .bind(self.billing_account_id)
        .fetch_all(executor)
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<models::billing_invoice::BillingInvoice>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as::<_, models::billing_invoice::BillingInvoice>(
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
        .fetch_all(executor)
        .await?;

        Ok(rows)
    }
}
