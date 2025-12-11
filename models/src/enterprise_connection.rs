use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};
use std::convert::TryFrom;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EnterpriseConnectionProtocol {
    Saml,
    Oidc,
}

impl ToString for EnterpriseConnectionProtocol {
    fn to_string(&self) -> String {
        match self {
            EnterpriseConnectionProtocol::Saml => "saml".to_string(),
            EnterpriseConnectionProtocol::Oidc => "oidc".to_string(),
        }
    }
}

impl TryFrom<String> for EnterpriseConnectionProtocol {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "saml" => Ok(Self::Saml),
            "oidc" => Ok(Self::Oidc),
            _ => Err(format!("Invalid protocol: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for EnterpriseConnectionProtocol {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("text")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for EnterpriseConnectionProtocol {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let s: &str = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Self::try_from(s.to_string()).map_err(|e| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for EnterpriseConnectionProtocol {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync + 'static>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&self.to_string(), buf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EnterpriseConnection {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub organization_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub domain_id: Option<i64>,
    
    #[sqlx(try_from = "String")]
    pub protocol: EnterpriseConnectionProtocol,
    pub idp_entity_id: Option<String>,
    pub idp_sso_url: Option<String>,
    pub idp_certificate: Option<String>,
    
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
