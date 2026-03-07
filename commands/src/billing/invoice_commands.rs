use chrono::{DateTime, Utc};
use common::error::AppError;
use models::billing_invoice::{BillingInvoice, InvoiceStatus};
use serde_json::Value;

pub struct UpsertInvoiceCommand {
    pub id: Option<i64>,
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
    pub fn new(
        id: i64,
        owner_id: String,
        provider_payment_id: String,
        provider_customer_id: String,
        amount_due_cents: i64,
        amount_paid_cents: i64,
        currency: String,
        status: String,
    ) -> Self {
        Self {
            id: Some(id),
            owner_id,
            provider_payment_id,
            provider_customer_id,
            amount_due_cents,
            amount_paid_cents,
            currency,
            status,
            invoice_pdf_url: None,
            hosted_invoice_url: None,
            invoice_number: None,
            due_date: None,
            paid_at: None,
            period_start: None,
            period_end: None,
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_invoice_pdf_url(mut self, invoice_pdf_url: Option<String>) -> Self {
        self.invoice_pdf_url = invoice_pdf_url;
        self
    }

    pub fn with_hosted_invoice_url(mut self, hosted_invoice_url: Option<String>) -> Self {
        self.hosted_invoice_url = hosted_invoice_url;
        self
    }

    pub fn with_invoice_number(mut self, invoice_number: Option<String>) -> Self {
        self.invoice_number = invoice_number;
        self
    }

    pub fn with_due_date(mut self, due_date: Option<DateTime<Utc>>) -> Self {
        self.due_date = due_date;
        self
    }

    pub fn with_paid_at(mut self, paid_at: Option<DateTime<Utc>>) -> Self {
        self.paid_at = paid_at;
        self
    }

    pub fn with_period_start(mut self, period_start: Option<DateTime<Utc>>) -> Self {
        self.period_start = period_start;
        self
    }

    pub fn with_period_end(mut self, period_end: Option<DateTime<Utc>>) -> Self {
        self.period_end = period_end;
        self
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub async fn execute_with_db<'a, A>(self, executor: A) -> Result<BillingInvoice, AppError>
    where
        A: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let invoice_id = self
            .id
            .ok_or_else(|| AppError::Validation("invoice_id is required".to_string()))?;

        let status: InvoiceStatus = self
            .status
            .parse()
            .map_err(|e| AppError::Validation(format!("Invalid invoice status: {}", e)))?;
        let row = sqlx::query!(
            r#"
            WITH account AS (
                SELECT id AS billing_account_id
                FROM billing_accounts
                WHERE owner_id = $1
            ),
            subscription AS (
                SELECT id AS subscription_id
                FROM subscriptions
                WHERE billing_account_id = (SELECT billing_account_id FROM account)
                LIMIT 1
            ),
            upsert AS (
                INSERT INTO billing_invoices (
                    id, billing_account_id, subscription_id, provider_payment_id,
                    provider_customer_id, amount_due_cents, amount_paid_cents,
                    currency, status, invoice_pdf_url, hosted_invoice_url,
                    invoice_number, due_date, paid_at, period_start, period_end,
                    attempt_count, metadata, created_at, updated_at
                )
                SELECT
                    $2,
                    (SELECT billing_account_id FROM account),
                    (SELECT subscription_id FROM subscription),
                    $3,
                    $4,
                    $5,
                    $6,
                    $7,
                    $8,
                    $9,
                    $10,
                    $11,
                    $12,
                    $13,
                    $14,
                    $15,
                    0,
                    $16,
                    NOW(),
                    NOW()
                WHERE EXISTS(SELECT 1 FROM account)
                ON CONFLICT (provider_payment_id) DO UPDATE SET
                    amount_due_cents = EXCLUDED.amount_due_cents,
                    amount_paid_cents = EXCLUDED.amount_paid_cents,
                    currency = EXCLUDED.currency,
                    status = EXCLUDED.status,
                    invoice_pdf_url = EXCLUDED.invoice_pdf_url,
                    hosted_invoice_url = EXCLUDED.hosted_invoice_url,
                    invoice_number = EXCLUDED.invoice_number,
                    due_date = EXCLUDED.due_date,
                    paid_at = EXCLUDED.paid_at,
                    period_start = EXCLUDED.period_start,
                    period_end = EXCLUDED.period_end,
                    metadata = EXCLUDED.metadata,
                    updated_at = NOW()
                RETURNING
                    id,
                    created_at,
                    updated_at,
                    billing_account_id,
                    subscription_id,
                    provider_payment_id,
                    provider_customer_id,
                    amount_due_cents,
                    amount_paid_cents,
                    currency,
                    status,
                    invoice_pdf_url,
                    hosted_invoice_url,
                    invoice_number,
                    due_date,
                    paid_at,
                    period_start,
                    period_end,
                    attempt_count,
                    next_payment_attempt,
                    metadata
            )
            SELECT
                EXISTS(SELECT 1 FROM account) AS "account_exists!",
                upsert.id,
                upsert.created_at,
                upsert.updated_at,
                upsert.billing_account_id,
                upsert.subscription_id,
                upsert.provider_payment_id,
                upsert.provider_customer_id,
                upsert.amount_due_cents,
                upsert.amount_paid_cents,
                upsert.currency,
                upsert.status,
                upsert.invoice_pdf_url,
                upsert.hosted_invoice_url,
                upsert.invoice_number,
                upsert.due_date,
                upsert.paid_at,
                upsert.period_start,
                upsert.period_end,
                upsert.attempt_count,
                upsert.next_payment_attempt,
                upsert.metadata
            FROM upsert
            UNION ALL
            SELECT
                EXISTS(SELECT 1 FROM account) AS "account_exists!",
                NULL::BIGINT AS id,
                NOW() AS created_at,
                NOW() AS updated_at,
                NULL::BIGINT AS billing_account_id,
                NULL::BIGINT AS subscription_id,
                NULL::TEXT AS provider_payment_id,
                NULL::TEXT AS provider_customer_id,
                NULL::BIGINT AS amount_due_cents,
                NULL::BIGINT AS amount_paid_cents,
                NULL::TEXT AS currency,
                NULL::TEXT AS status,
                NULL::TEXT AS invoice_pdf_url,
                NULL::TEXT AS hosted_invoice_url,
                NULL::TEXT AS invoice_number,
                NULL::TIMESTAMPTZ AS due_date,
                NULL::TIMESTAMPTZ AS paid_at,
                NULL::TIMESTAMPTZ AS period_start,
                NULL::TIMESTAMPTZ AS period_end,
                NULL::INT AS attempt_count,
                NULL::TIMESTAMPTZ AS next_payment_attempt,
                NULL::JSONB AS metadata
            WHERE NOT EXISTS(SELECT 1 FROM upsert)
            LIMIT 1
            "#,
            self.owner_id,
            invoice_id,
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
        .fetch_one(executor)
        .await?;

        if !row.account_exists {
            return Err(AppError::Validation(
                "Billing account not found".to_string(),
            ));
        }

        let parsed_status: InvoiceStatus = row
            .status
            .as_deref()
            .unwrap_or("draft")
            .parse()
            .unwrap_or(InvoiceStatus::Draft);

        Ok(BillingInvoice {
            id: row.id.unwrap_or(invoice_id),
            created_at: row.created_at.unwrap_or_else(Utc::now),
            updated_at: row.updated_at.unwrap_or_else(Utc::now),
            billing_account_id: row.billing_account_id.unwrap_or_default(),
            subscription_id: row.subscription_id,
            provider_payment_id: row.provider_payment_id.unwrap_or_default(),
            provider_customer_id: row.provider_customer_id.unwrap_or_default(),
            amount_due_cents: row.amount_due_cents.unwrap_or_default(),
            amount_paid_cents: row.amount_paid_cents.unwrap_or_default(),
            currency: row.currency.unwrap_or_default(),
            status: parsed_status,
            invoice_pdf_url: row.invoice_pdf_url,
            hosted_invoice_url: row.hosted_invoice_url,
            invoice_number: row.invoice_number,
            due_date: row.due_date,
            paid_at: row.paid_at,
            period_start: row.period_start,
            period_end: row.period_end,
            attempt_count: row.attempt_count.unwrap_or(0),
            next_payment_attempt: row.next_payment_attempt,
            metadata: row.metadata.unwrap_or_else(|| serde_json::json!({})),
        })
    }
}
