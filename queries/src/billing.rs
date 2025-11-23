use crate::Query;
use chrono::NaiveDate;
use common::error::AppError;
use common::state::AppState;
use models::billing::{BillingAccount, BillingAccountWithSubscription, Subscription};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

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
}

impl Query for GetBillingAccountQuery {
    type Output = Option<BillingAccountWithSubscription>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        // First get the billing account
        let row = sqlx::query!(
            r#"
            SELECT
                id, owner_id, owner_type, provider_customer_id, legal_name, tax_id, billing_email, billing_phone,
                address_line1, address_line2, city, state, postal_code, country, status,
                payment_method_status, currency, locale, created_at, updated_at
            FROM billing_accounts WHERE owner_id = $1
            "#,
            &self.owner_id
        )
        .fetch_optional(&state.db_pool)
        .await?;

        let billing_account = row.map(|r| BillingAccount {
            id: r.id,
            owner_id: r.owner_id,
            owner_type: r.owner_type,
            provider_customer_id: r.provider_customer_id,
            legal_name: r.legal_name,
            tax_id: r.tax_id,
            billing_email: r.billing_email,
            billing_phone: r.billing_phone,
            address_line1: r.address_line1,
            address_line2: r.address_line2,
            city: r.city,
            state: r.state,
            postal_code: r.postal_code,
            country: r.country,
            status: r.status,
            payment_method_status: r.payment_method_status,
            currency: r.currency.unwrap_or_else(|| "USD".to_string()),
            locale: r.locale.unwrap_or_else(|| "en-US".to_string()),
            created_at: r.created_at,
            updated_at: r.updated_at,
        });

        if let Some(account) = billing_account {
            // Then get the subscription if it exists
            let subscription = sqlx::query_as!(
                Subscription,
                r#"
                SELECT * FROM subscriptions WHERE billing_account_id = $1
                "#,
                account.id
            )
            .fetch_optional(&state.db_pool)
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
}

impl Query for GetSubscriptionByProviderIdQuery {
    type Output = Option<BillingAccountWithSubscription>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            SELECT * FROM subscriptions WHERE provider_subscription_id = $1
            "#,
            &self.provider_subscription_id
        )
        .fetch_optional(&state.db_pool)
        .await?;

        if let Some(sub) = subscription {
            let row = sqlx::query!(
                r#"
                SELECT
                    id, owner_id, owner_type, provider_customer_id, legal_name, tax_id, billing_email, billing_phone,
                    address_line1, address_line2, city, state, postal_code, country, status,
                    payment_method_status, currency, locale, created_at, updated_at
                FROM billing_accounts WHERE id = $1
                "#,
                sub.billing_account_id
            )
            .fetch_one(&state.db_pool)
            .await?;

            let billing_account = BillingAccount {
                id: row.id,
                owner_id: row.owner_id,
                owner_type: row.owner_type,
                provider_customer_id: row.provider_customer_id,
                legal_name: row.legal_name,
                tax_id: row.tax_id,
                billing_email: row.billing_email,
                billing_phone: row.billing_phone,
                address_line1: row.address_line1,
                address_line2: row.address_line2,
                city: row.city,
                state: row.state,
                postal_code: row.postal_code,
                country: row.country,
                status: row.status,
                payment_method_status: row.payment_method_status,
                currency: row.currency.unwrap_or_else(|| "USD".to_string()),
                locale: row.locale.unwrap_or_else(|| "en-US".to_string()),
                created_at: row.created_at,
                updated_at: row.updated_at,
            };

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
}

pub struct GetDeploymentProviderSubscriptionQuery {
    deployment_id: i64,
}

impl GetDeploymentProviderSubscriptionQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentProviderSubscriptionQuery {
    type Output = Option<ProviderSubscriptionInfo>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT s.provider_customer_id, s.provider_subscription_id
            FROM deployments d
            JOIN projects p ON d.project_id = p.id
            JOIN billing_accounts ba ON p.billing_account_id = ba.id
            JOIN subscriptions s ON s.billing_account_id = ba.id
            WHERE d.id = $1 AND s.status = 'active'
            LIMIT 1
            "#,
            self.deployment_id
        )
        .fetch_optional(&state.db_pool)
        .await?;

        Ok(row.map(|r| ProviderSubscriptionInfo {
            provider_customer_id: r.provider_customer_id,
            provider_subscription_id: r.provider_subscription_id,
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
    pub billing_period: NaiveDate,
}

impl GetDeploymentUsageQuery {
    pub fn new(deployment_id: i64, billing_period: NaiveDate) -> Self {
        Self {
            deployment_id,
            billing_period,
        }
    }
}

impl Query for GetDeploymentUsageQuery {
    type Output = Vec<UsageSnapshot>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_all(&state.db_pool)
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
}

impl Query for GetDodoProductQuery {
    type Output = Option<DodoProduct>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, plan_name, product_id, display_name, description, base_price_cents, is_active
            FROM dodo_products
            WHERE plan_name = $1 AND is_active = true
            "#,
            &self.plan_name
        )
        .fetch_optional(&state.db_pool)
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
}

impl Query for GetBillingAccountByProviderCustomerIdQuery {
    type Output = Option<String>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let owner_id = sqlx::query_scalar!(
            "SELECT owner_id FROM billing_accounts WHERE provider_customer_id = $1",
            &self.provider_customer_id
        )
        .fetch_optional(&state.db_pool)
        .await?;

        Ok(owner_id)
    }
}

pub struct GetAllDodoProductsQuery;

impl Query for GetAllDodoProductsQuery {
    type Output = Vec<DodoProduct>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let rows = sqlx::query!(
            r#"
            SELECT id, plan_name, product_id, display_name, description, base_price_cents, is_active
            FROM dodo_products
            WHERE is_active = true
            ORDER BY base_price_cents ASC
            "#
        )
        .fetch_all(&state.db_pool)
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
