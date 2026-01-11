use crate::Command;
use chrono::{DateTime, NaiveDate, Utc};
use common::error::AppError;
use common::state::AppState;
use models::billing::Subscription;
use models::billing_invoice::{BillingInvoice, InvoiceStatus};
use rust_decimal::Decimal;
use serde_json::Value;

pub struct CreateBillingAccountCommand {
    pub owner_id: String,
    pub owner_type: String,
    pub legal_name: String,
    pub billing_email: String,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
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
            self.address_line1.as_deref().unwrap_or(""),
            self.address_line2,
            self.city.as_deref().unwrap_or(""),
            self.state,
            self.postal_code.as_deref().unwrap_or(""),
            self.country.as_deref().unwrap_or("US")
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

pub struct UpdateBillingAccountFromWebhookCommand {
    pub owner_id: String,
    pub legal_name: Option<String>,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub company: Option<String>,
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
    pub provider_customer_id: String,
    pub provider_subscription_id: String,
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
                provider_customer_id,
                provider_subscription_id,
                status,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
            RETURNING *
            "#,
            id,
            self.billing_account_id,
            self.provider_customer_id,
            self.provider_subscription_id,
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

pub struct SetProviderCustomerIdCommand {
    pub owner_id: String,
    pub provider_customer_id: String,
}

impl Command for SetProviderCustomerIdCommand {
    type Output = ();

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET provider_customer_id = $1, updated_at = NOW()
            WHERE owner_id = $2
            "#,
            self.provider_customer_id,
            self.owner_id
        )
        .execute(&state.db_pool)
        .await?;

        Ok(())
    }
}

impl Command for UpdateBillingAccountFromWebhookCommand {
    type Output = ();

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        // Simple approach: just update all fields that are provided
        // Use COALESCE to keep existing values if new value is NULL
        sqlx::query!(
            r#"
            UPDATE billing_accounts 
            SET 
                legal_name = COALESCE($1, legal_name),
                billing_email = COALESCE($2, billing_email),
                billing_phone = COALESCE($3, billing_phone),
                address_line1 = COALESCE($4, address_line1),
                address_line2 = COALESCE($5, address_line2),
                city = COALESCE($6, city),
                state = COALESCE($7, state),
                postal_code = COALESCE($8, postal_code),
                country = COALESCE($9, country),
                updated_at = NOW()
            WHERE owner_id = $10
            "#,
            self.legal_name,
            self.billing_email,
            self.billing_phone,
            self.address_line1,
            self.address_line2,
            self.city,
            self.state,
            self.postal_code,
            self.country,
            self.owner_id
        )
        .execute(&state.db_pool)
        .await?;

        Ok(())
    }
}

pub struct UpsertSubscriptionCommand {
    pub owner_id: String,
    pub provider_customer_id: String,
    pub provider_subscription_id: String,
    pub product_id: Option<String>,
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
                    provider_customer_id = $1,
                    provider_subscription_id = $2,
                    product_id = $3,
                    status = $4,
                    updated_at = NOW()
                WHERE id = $5
                RETURNING *
                "#,
                self.provider_customer_id,
                self.provider_subscription_id,
                self.product_id,
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
                    provider_customer_id,
                    provider_subscription_id,
                    product_id,
                    status,
                    created_at,
                    updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
                RETURNING *
                "#,
                id,
                billing_account_id,
                self.provider_customer_id,
                self.provider_subscription_id,
                self.product_id,
                self.status
            )
            .fetch_one(&state.db_pool)
            .await?
        };

        Ok(subscription)
    }
}

pub struct UpsertInvoiceCommand {
    pub owner_id: String,
    pub provider_payment_id: String,
    pub provider_customer_id: String,
    pub amount_due_cents: i64,
    pub amount_paid_cents: i64,
    pub currency: String,
    pub status: String,
    pub invoice_pdf_url: Option<String>,
    pub hosted_invoice_url: Option<String>,
    pub invoice_number: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub metadata: Value,
}

impl Command for UpsertInvoiceCommand {
    type Output = BillingInvoice;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let billing_account_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM billing_accounts WHERE owner_id = $1",
            self.owner_id
        )
        .fetch_optional(&state.db_pool)
        .await?;

        let billing_account_id = billing_account_id
            .ok_or_else(|| AppError::Validation("Billing account not found".to_string()))?;

        let subscription_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM subscriptions WHERE billing_account_id = $1",
            billing_account_id
        )
        .fetch_optional(&state.db_pool)
        .await?;

        let status: InvoiceStatus = self
            .status
            .parse()
            .map_err(|e| AppError::Validation(format!("Invalid invoice status: {}", e)))?;

        let existing_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM billing_invoices WHERE provider_payment_id = $1",
            self.provider_payment_id
        )
        .fetch_optional(&state.db_pool)
        .await?;

        let invoice = if let Some(id) = existing_id {
            sqlx::query_as!(
                BillingInvoice,
                r#"
                UPDATE billing_invoices SET
                    amount_due_cents = $1,
                    amount_paid_cents = $2,
                    currency = $3,
                    status = $4,
                    invoice_pdf_url = $5,
                    hosted_invoice_url = $6,
                    invoice_number = $7,
                    due_date = $8,
                    paid_at = $9,
                    period_start = $10,
                    period_end = $11,
                    metadata = $12,
                    updated_at = NOW()
                WHERE id = $13
                RETURNING
                    id, created_at, updated_at, billing_account_id, subscription_id,
                    provider_payment_id, provider_customer_id, amount_due_cents,
                    amount_paid_cents, currency, status as "status: InvoiceStatus",
                    invoice_pdf_url, hosted_invoice_url, invoice_number, due_date,
                    paid_at, period_start, period_end, attempt_count, next_payment_attempt,
                    metadata
                "#,
                self.amount_due_cents,
                self.amount_paid_cents,
                self.currency,
                status.to_string(),
                self.invoice_pdf_url,
                self.hosted_invoice_url,
                self.invoice_number,
                self.due_date,
                self.paid_at,
                self.period_start,
                self.period_end,
                self.metadata,
                id
            )
            .fetch_one(&state.db_pool)
            .await?
        } else {
            let id = state.sf.next_id().unwrap() as i64;
            sqlx::query_as!(
                BillingInvoice,
                r#"
                INSERT INTO billing_invoices (
                    id, billing_account_id, subscription_id, provider_payment_id,
                    provider_customer_id, amount_due_cents, amount_paid_cents,
                    currency, status, invoice_pdf_url, hosted_invoice_url,
                    invoice_number, due_date, paid_at, period_start, period_end,
                    attempt_count, metadata, created_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, 0, $17, NOW(), NOW())
                RETURNING
                    id, created_at, updated_at, billing_account_id, subscription_id,
                    provider_payment_id, provider_customer_id, amount_due_cents,
                    amount_paid_cents, currency, status as "status: InvoiceStatus",
                    invoice_pdf_url, hosted_invoice_url, invoice_number, due_date,
                    paid_at, period_start, period_end, attempt_count, next_payment_attempt,
                    metadata
                "#,
                id,
                billing_account_id,
                subscription_id,
                self.provider_payment_id,
                self.provider_customer_id,
                self.amount_due_cents,
                self.amount_paid_cents,
                self.currency,
                status.to_string(),
                self.invoice_pdf_url,
                self.hosted_invoice_url,
                self.invoice_number,
                self.due_date,
                self.paid_at,
                self.period_start,
                self.period_end,
                self.metadata
            )
            .fetch_one(&state.db_pool)
            .await?
        };

        Ok(invoice)
    }
}

pub struct CreateBillingSyncRunCommand {
    pub from_event_id: i64,
}

impl Command for CreateBillingSyncRunCommand {
    type Output = i64;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let rec = sqlx::query!(
            "INSERT INTO billing_sync_runs (from_event_id, to_event_id, status)
             VALUES ($1, 0, 'running')
             RETURNING id",
            self.from_event_id
        )
        .fetch_one(&state.db_pool)
        .await?;

        Ok(rec.id)
    }
}

pub struct CompleteBillingSyncRunCommand {
    pub sync_run_id: i64,
    pub events_processed: i64,
    pub deployments_affected: i32,
}

impl Command for CompleteBillingSyncRunCommand {
    type Output = ();

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "UPDATE billing_sync_runs
             SET completed_at = NOW(),
                 events_processed = $2,
                 deployments_affected = $3,
                 status = 'completed'
             WHERE id = $1",
            self.sync_run_id,
            self.events_processed,
            self.deployments_affected
        )
        .execute(&state.db_pool)
        .await?;

        Ok(())
    }
}

pub struct UpsertUsageSnapshotCommand {
    pub deployment_id: i64,
    pub billing_period: NaiveDate,
    pub metric_name: String,
    pub quantity: i64,
    pub cost_cents: Option<Decimal>,
}

impl Command for UpsertUsageSnapshotCommand {
    type Output = ();

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "INSERT INTO billing_usage_snapshots
             (deployment_id, billing_period, metric_name, quantity, cost_cents, min_event_id, max_event_id)
             VALUES ($1, $2, $3, $4, $5, 0, 0)
             ON CONFLICT (deployment_id, billing_period, metric_name)
             DO UPDATE SET
                quantity = billing_usage_snapshots.quantity + $4,
                cost_cents = COALESCE(billing_usage_snapshots.cost_cents, 0) + COALESCE($5, 0),
                updated_at = NOW()",
            self.deployment_id,
            self.billing_period,
            self.metric_name,
            self.quantity,
            self.cost_cents
        )
        .execute(&state.db_pool)
        .await?;

        Ok(())
    }
}
