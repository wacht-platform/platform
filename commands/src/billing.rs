use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::billing::Subscription;

pub struct CreateBillingAccountCommand {
    pub owner_id: String,
    pub owner_type: String,
    pub legal_name: String,
    pub billing_email: String,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub address_line1: String,
    pub address_line2: Option<String>,
    pub city: String,
    pub state: Option<String>,
    pub postal_code: String,
    pub country: String,
}

impl Command for CreateBillingAccountCommand {
    type Output = i64;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let id = state.sf.next_id().unwrap() as i64;

        sqlx::query!(
            r#"
            INSERT INTO billing_accounts (
                id,
                owner_id,
                owner_type,
                legal_name,
                billing_email,
                billing_phone,
                tax_id,
                address_line1,
                address_line2,
                city,
                state,
                postal_code,
                country,
                status,
                currency,
                locale,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'pending', 'USD', 'en-US', NOW(), NOW())
            "#,
            id,
            self.owner_id,
            self.owner_type,
            self.legal_name,
            self.billing_email,
            self.billing_phone,
            self.tax_id,
            self.address_line1,
            self.address_line2,
            self.city,
            self.state,
            self.postal_code,
            self.country
        )
        .execute(&state.db_pool)
        .await?;

        Ok(id)
    }
}

pub struct UpdateBillingAccountCommand {
    pub id: i64,
    pub legal_name: Option<String>,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}

impl Command for UpdateBillingAccountCommand {
    type Output = ();

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let mut query = String::from("UPDATE billing_accounts SET updated_at = NOW()");
        let mut params: Vec<String> = Vec::new();
        let mut param_count = 1;

        if let Some(legal_name) = self.legal_name {
            query.push_str(&format!(", legal_name = ${}", param_count));
            params.push(legal_name);
            param_count += 1;
        }

        if let Some(billing_email) = self.billing_email {
            query.push_str(&format!(", billing_email = ${}", param_count));
            params.push(billing_email);
            param_count += 1;
        }

        if let Some(billing_phone) = self.billing_phone {
            query.push_str(&format!(", billing_phone = ${}", param_count));
            params.push(billing_phone);
            param_count += 1;
        }

        if let Some(tax_id) = self.tax_id {
            query.push_str(&format!(", tax_id = ${}", param_count));
            params.push(tax_id);
            param_count += 1;
        }

        if let Some(address_line1) = self.address_line1 {
            query.push_str(&format!(", address_line1 = ${}", param_count));
            params.push(address_line1);
            param_count += 1;
        }

        if let Some(address_line2) = self.address_line2 {
            query.push_str(&format!(", address_line2 = ${}", param_count));
            params.push(address_line2);
            param_count += 1;
        }

        if let Some(city) = self.city {
            query.push_str(&format!(", city = ${}", param_count));
            params.push(city);
            param_count += 1;
        }

        if let Some(state) = self.state {
            query.push_str(&format!(", state = ${}", param_count));
            params.push(state);
            param_count += 1;
        }

        if let Some(postal_code) = self.postal_code {
            query.push_str(&format!(", postal_code = ${}", param_count));
            params.push(postal_code);
            param_count += 1;
        }

        if let Some(country) = self.country {
            query.push_str(&format!(", country = ${}", param_count));
            params.push(country);
            param_count += 1;
        }

        query.push_str(&format!(" WHERE id = ${}", param_count));

        sqlx::query(&query)
            .bind(self.id)
            .execute(&state.db_pool)
            .await?;

        Ok(())
    }
}

pub struct CreateSubscriptionCommand {
    pub billing_account_id: i64,
    pub chargebee_customer_id: String,
    pub chargebee_subscription_id: String,
    pub status: String,
}

impl Command for CreateSubscriptionCommand {
    type Output = Subscription;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let id = state.sf.next_id().unwrap() as i64;

        let subscription = sqlx::query_as!(
            Subscription,
            r#"
            INSERT INTO subscriptions (
                id,
                billing_account_id,
                chargebee_customer_id,
                chargebee_subscription_id,
                status,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
            RETURNING *
            "#,
            id,
            self.billing_account_id,
            self.chargebee_customer_id,
            self.chargebee_subscription_id,
            self.status
        )
        .fetch_one(&state.db_pool)
        .await?;

        Ok(subscription)
    }
}

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

pub struct UpdateBillingAccountStatusCommand {
    pub owner_id: String,
    pub status: String,
}

impl Command for UpdateBillingAccountStatusCommand {
    type Output = ();

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            r#"
            UPDATE billing_accounts 
            SET status = $1, updated_at = NOW()
            WHERE owner_id = $2
            "#,
            self.status,
            self.owner_id
        )
        .execute(&state.db_pool)
        .await?;

        Ok(())
    }
}

pub struct UpsertSubscriptionCommand {
    pub owner_id: String,
    pub chargebee_customer_id: String,
    pub chargebee_subscription_id: String,
    pub status: String,
}

impl Command for UpsertSubscriptionCommand {
    type Output = Subscription;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let billing_account_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM billing_accounts WHERE owner_id = $1",
            self.owner_id
        )
        .fetch_optional(&state.db_pool)
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
        .fetch_optional(&state.db_pool)
        .await?;

        let subscription = if let Some(id) = existing_id {
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
            let id = state.sf.next_id().unwrap() as i64;
            sqlx::query_as!(
                Subscription,
                r#"
                INSERT INTO subscriptions (
                    id,
                    billing_account_id,
                    chargebee_customer_id,
                    chargebee_subscription_id,
                    status,
                    created_at,
                    updated_at
                ) VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
                RETURNING *
                "#,
                id,
                billing_account_id,
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
