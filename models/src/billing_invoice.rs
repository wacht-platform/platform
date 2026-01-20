use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    Draft,
    Open,
    Paid,
    Uncollectible,
    Void,
}

impl std::fmt::Display for InvoiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvoiceStatus::Draft => write!(f, "draft"),
            InvoiceStatus::Open => write!(f, "open"),
            InvoiceStatus::Paid => write!(f, "paid"),
            InvoiceStatus::Uncollectible => write!(f, "uncollectible"),
            InvoiceStatus::Void => write!(f, "void"),
        }
    }
}

impl std::str::FromStr for InvoiceStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(InvoiceStatus::Draft),
            "open" => Ok(InvoiceStatus::Open),
            "paid" => Ok(InvoiceStatus::Paid),
            "uncollectible" => Ok(InvoiceStatus::Uncollectible),
            "void" => Ok(InvoiceStatus::Void),
            _ => Err(format!("Invalid invoice status: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for InvoiceStatus {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for InvoiceStatus {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        value.parse().map_err(|e: String| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for InvoiceStatus {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct BillingInvoice {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub billing_account_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub subscription_id: Option<i64>,
    pub provider_payment_id: String,
    pub provider_customer_id: String,
    pub amount_due_cents: i64,
    pub amount_paid_cents: i64,
    pub currency: String,
    pub status: InvoiceStatus,
    pub invoice_pdf_url: Option<String>,
    pub hosted_invoice_url: Option<String>,
    pub invoice_number: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub attempt_count: i32,
    pub next_payment_attempt: Option<DateTime<Utc>>,
    pub metadata: Value,
}

impl BillingInvoice {
    pub fn amount_due_in_currency_unit(&self) -> f64 {
        self.amount_due_cents as f64 / 100.0
    }

    pub fn amount_paid_in_currency_unit(&self) -> f64 {
        self.amount_paid_cents as f64 / 100.0
    }

    pub fn amount_remaining_cents(&self) -> i64 {
        self.amount_due_cents - self.amount_paid_cents
    }

    pub fn amount_remaining_in_currency_unit(&self) -> f64 {
        self.amount_remaining_cents() as f64 / 100.0
    }

    pub fn is_paid(&self) -> bool {
        self.status == InvoiceStatus::Paid
    }

    pub fn is_overdue(&self) -> bool {
        if let Some(due_date) = self.due_date {
            let now = Utc::now();
            due_date < now && !self.is_paid()
        } else {
            false
        }
    }

    pub fn days_until_due(&self) -> Option<i64> {
        self.due_date.map(|due| {
            let now = Utc::now();
            (due - now).num_days()
        })
    }

    pub fn is_partially_paid(&self) -> bool {
        self.amount_paid_cents > 0 && self.amount_paid_cents < self.amount_due_cents
    }
}
