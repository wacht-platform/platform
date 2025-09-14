use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;

use super::SecondFactorPolicy;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStrategy {
    Otp,
    OauthGoogle,
    OauthGithub,
    OauthMicrosoft,
    OauthFacebook,
    OauthLinkedin,
    OauthDiscord,
    OauthApple,
}

impl FromStr for VerificationStrategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "otp" => Ok(VerificationStrategy::Otp),
            "oauth_google" => Ok(VerificationStrategy::OauthGoogle),
            "oauth_github" => Ok(VerificationStrategy::OauthGithub),
            "oauth_microsoft" => Ok(VerificationStrategy::OauthMicrosoft),
            "oauth_facebook" => Ok(VerificationStrategy::OauthFacebook),
            "oauth_linkedin" => Ok(VerificationStrategy::OauthLinkedin),
            "oauth_discord" => Ok(VerificationStrategy::OauthDiscord),
            "oauth_apple" => Ok(VerificationStrategy::OauthApple),
            _ => Err(format!("Invalid verification strategy: {}", s)),
        }
    }
}

impl ToString for VerificationStrategy {
    fn to_string(&self) -> String {
        match self {
            VerificationStrategy::Otp => "otp".to_string(),
            VerificationStrategy::OauthGoogle => "oauth_google".to_string(),
            VerificationStrategy::OauthGithub => "oauth_github".to_string(),
            VerificationStrategy::OauthMicrosoft => "oauth_microsoft".to_string(),
            VerificationStrategy::OauthFacebook => "oauth_facebook".to_string(),
            VerificationStrategy::OauthLinkedin => "oauth_linkedin".to_string(),
            VerificationStrategy::OauthDiscord => "oauth_discord".to_string(),
            VerificationStrategy::OauthApple => "oauth_apple".to_string(),
        }
    }
}

// Implement sqlx::Type and sqlx::Decode for VerificationStrategy
impl sqlx::Type<sqlx::Postgres> for VerificationStrategy {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for VerificationStrategy {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        VerificationStrategy::from_str(value).map_err(|e| e.into())
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SchemaVersion {
    V1,
    V2,
}

impl FromStr for SchemaVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "v1" => Ok(SchemaVersion::V1),
            "v2" => Ok(SchemaVersion::V2),
            _ => Err(format!("Invalid schema version: {}", s)),
        }
    }
}

impl ToString for SchemaVersion {
    fn to_string(&self) -> String {
        match self {
            SchemaVersion::V1 => "v1".to_string(),
            SchemaVersion::V2 => "v2".to_string(),
        }
    }
}

// Implement sqlx::Type and sqlx::Decode for SchemaVersion
impl sqlx::Type<sqlx::Postgres> for SchemaVersion {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SchemaVersion {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        SchemaVersion::from_str(value).map_err(|e| e.into())
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserEmailAddress {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deployment_id: i64,
    pub user_id: i64,
    pub email: String,
    pub is_primary: bool,
    pub verified: bool,
    pub verified_at: DateTime<Utc>,
    pub verification_strategy: VerificationStrategy,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_name: String,
    pub last_name: String,
    pub username: String,
    pub password: String,
    pub profile_picture_url: String,
    pub schema_version: SchemaVersion,
    pub disabled: bool,
    pub primary_email_address_id: Option<i64>,
    pub primary_phone_number_id: Option<i64>,
    pub second_factor_policy: SecondFactorPolicy,
    pub active_organization_membership_id: Option<i64>,
    pub active_workspace_membership_id: Option<i64>,
    pub deployment_id: i64,
    pub public_metadata: Value,
    pub private_metadata: Value,
    pub backup_codes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserWithIdentifiers {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_name: String,
    pub last_name: String,
    pub username: Option<String>,
    pub profile_picture_url: String,
    pub primary_email_address: Option<String>,
    pub primary_phone_number: Option<String>,
}
