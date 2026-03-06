use chrono::{DateTime, Utc};
use common::error::AppError;
use models::billing_invoice::{BillingInvoice, InvoiceStatus};
use serde_json::Value;

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

impl UpsertInvoiceCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        invoice_id: i64,
    ) -> Result<BillingInvoice, AppError>
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

        let billing_account_id = billing_account_id
            .ok_or_else(|| AppError::Validation("Billing account not found".to_string()))?;

        let subscription_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM subscriptions WHERE billing_account_id = $1",
            billing_account_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        let status: InvoiceStatus = self
            .status
            .parse()
            .map_err(|e| AppError::Validation(format!("Invalid invoice status: {}", e)))?;

        let existing_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM billing_invoices WHERE provider_payment_id = $1",
            self.provider_payment_id
        )
        .fetch_optional(&mut *conn)
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
            .fetch_one(&mut *conn)
            .await?
        } else {
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
                invoice_id,
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
            .fetch_one(&mut *conn)
            .await?
        };

        Ok(invoice)
    }
}
