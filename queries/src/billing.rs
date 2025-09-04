use crate::Query;
use common::error::AppError;
use common::state::AppState;
use models::billing::{BillingAccount, BillingAccountWithSubscription, Subscription};

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
                id, owner_id, owner_type, legal_name, tax_id, billing_email, billing_phone,
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

pub struct GetSubscriptionByChargebeeIdQuery {
    chargebee_subscription_id: String,
}

impl GetSubscriptionByChargebeeIdQuery {
    pub fn new(chargebee_subscription_id: String) -> Self {
        Self {
            chargebee_subscription_id,
        }
    }
}

impl Query for GetSubscriptionByChargebeeIdQuery {
    type Output = Option<BillingAccountWithSubscription>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        // First get the subscription
        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            SELECT * FROM subscriptions WHERE chargebee_subscription_id = $1
            "#,
            &self.chargebee_subscription_id
        )
        .fetch_optional(&state.db_pool)
        .await?;

        if let Some(sub) = subscription {
            // Then get the billing account
            let row = sqlx::query!(
                r#"
                SELECT 
                    id, owner_id, owner_type, legal_name, tax_id, billing_email, billing_phone,
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
