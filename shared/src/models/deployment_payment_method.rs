use chrono::{DateTime, Utc, Datelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethodType {
    Card,
    BankAccount,
}

impl std::fmt::Display for PaymentMethodType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaymentMethodType::Card => write!(f, "card"),
            PaymentMethodType::BankAccount => write!(f, "bank_account"),
        }
    }
}

impl std::str::FromStr for PaymentMethodType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "card" => Ok(PaymentMethodType::Card),
            "bank_account" => Ok(PaymentMethodType::BankAccount),
            _ => Err(format!("Invalid payment method type: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for PaymentMethodType {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for PaymentMethodType {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        value.parse().map_err(|e: String| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for PaymentMethodType {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentPaymentMethod {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub user_id: Option<i64>,
    pub stripe_payment_method_id: String,
    pub stripe_customer_id: String,
    pub payment_method_type: PaymentMethodType,
    pub is_default: bool,
    pub card_brand: Option<String>,
    pub card_last4: Option<String>,
    pub card_exp_month: Option<i32>,
    pub card_exp_year: Option<i32>,
    pub bank_name: Option<String>,
    pub bank_last4: Option<String>,
    pub metadata: Value,
}

impl DeploymentPaymentMethod {
    pub fn display_name(&self) -> String {
        match self.payment_method_type {
            PaymentMethodType::Card => {
                let brand = self.card_brand.as_deref().unwrap_or("Card");
                let last4 = self.card_last4.as_deref().unwrap_or("****");
                format!("{} ending in {}", brand, last4)
            }
            PaymentMethodType::BankAccount => {
                let bank = self.bank_name.as_deref().unwrap_or("Bank");
                let last4 = self.bank_last4.as_deref().unwrap_or("****");
                format!("{} ending in {}", bank, last4)
            }
        }
    }

    pub fn is_card(&self) -> bool {
        self.payment_method_type == PaymentMethodType::Card
    }

    pub fn is_bank_account(&self) -> bool {
        self.payment_method_type == PaymentMethodType::BankAccount
    }

    pub fn is_expired(&self) -> bool {
        if let (Some(exp_month), Some(exp_year)) = (self.card_exp_month, self.card_exp_year) {
            let now = Utc::now();
            let current_year = now.year() as i32;
            let current_month = now.month() as i32;
            
            exp_year < current_year || (exp_year == current_year && exp_month < current_month)
        } else {
            false
        }
    }

    pub fn expires_soon(&self) -> bool {
        if let (Some(exp_month), Some(exp_year)) = (self.card_exp_month, self.card_exp_year) {
            let now = Utc::now();
            let current_year = now.year() as i32;
            let current_month = now.month() as i32;
            
            // Check if expires within 3 months
            let months_until_expiry = (exp_year - current_year) * 12 + (exp_month - current_month);
            months_until_expiry <= 3 && months_until_expiry > 0
        } else {
            false
        }
    }
}