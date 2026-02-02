use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgTypeInfo;

use crate::error::AppError;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum FirstFactor {
    EmailPassword,
    UsernamePassword,
    EmailOtp,
    EmailMagicLink,
    PhoneOtp,
}

impl FromStr for FirstFactor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "email_password" => Ok(FirstFactor::EmailPassword),
            "username_password" => Ok(FirstFactor::UsernamePassword),
            "email_otp" => Ok(FirstFactor::EmailOtp),
            "email_magic_link" => Ok(FirstFactor::EmailMagicLink),
            "phone_otp" => Ok(FirstFactor::PhoneOtp),
            _ => Err(format!("Invalid first factor: {}", s)),
        }
    }
}

impl ToString for FirstFactor {
    fn to_string(&self) -> String {
        match self {
            FirstFactor::EmailPassword => "email_password".to_string(),
            FirstFactor::UsernamePassword => "username_password".to_string(),
            FirstFactor::EmailOtp => "email_otp".to_string(),
            FirstFactor::EmailMagicLink => "email_magic_link".to_string(),
            FirstFactor::PhoneOtp => "phone_otp".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SecondFactor {
    None,
    PhoneOtp,
    BackupCode,
    Authenticator,
}

impl FromStr for SecondFactor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(SecondFactor::None),
            "phone_otp" => Ok(SecondFactor::PhoneOtp),
            "backup_code" => Ok(SecondFactor::BackupCode),
            "authenticator" => Ok(SecondFactor::Authenticator),
            _ => Err(format!("Invalid second factor: {}", s)),
        }
    }
}

impl ToString for SecondFactor {
    fn to_string(&self) -> String {
        match self {
            SecondFactor::None => "none".to_string(),
            SecondFactor::PhoneOtp => "phone_otp".to_string(),
            SecondFactor::BackupCode => "backup_code".to_string(),
            SecondFactor::Authenticator => "authenticator".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SecondFactorPolicy {
    None,
    Optional,
    Enforced,
}

impl FromStr for SecondFactorPolicy {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(SecondFactorPolicy::None),
            "optional" => Ok(SecondFactorPolicy::Optional),
            "enforced" => Ok(SecondFactorPolicy::Enforced),
            _ => Err(AppError::Serialization(format!(
                "Invalid second factor policy: {}",
                s
            ))),
        }
    }
}

impl ToString for SecondFactorPolicy {
    fn to_string(&self) -> String {
        match self {
            SecondFactorPolicy::None => "none".to_string(),
            SecondFactorPolicy::Optional => "optional".to_string(),
            SecondFactorPolicy::Enforced => "enforced".to_string(),
        }
    }
}

// Implement sqlx::Type and sqlx::Decode for SecondFactorPolicy
impl sqlx::Type<sqlx::Postgres> for SecondFactorPolicy {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SecondFactorPolicy {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        SecondFactorPolicy::from_str(value).map_err(|e| e.into())
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FirstFactorPolicy {
    None,
    Optional,
    Enforced,
}

impl FromStr for FirstFactorPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(FirstFactorPolicy::None),
            "optional" => Ok(FirstFactorPolicy::Optional),
            "enforced" => Ok(FirstFactorPolicy::Enforced),
            _ => Err(format!("Invalid first factor policy: {}", s)),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IndividualAuthSettings {
    pub enabled: bool,
    pub required: Option<bool>,
}

impl Default for IndividualAuthSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            required: Some(false),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PasswordSettings {
    pub enabled: bool,
    pub min_length: Option<u8>,
    pub require_lowercase: Option<bool>,
    pub require_uppercase: Option<bool>,
    pub require_number: Option<bool>,
    pub require_special: Option<bool>,
}

impl Default for PasswordSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            min_length: Some(8),
            require_lowercase: Some(true),
            require_uppercase: Some(true),
            require_number: Some(true),
            require_special: Some(true),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct VerificationPolicy {
    pub phone_number: bool,
    pub email: bool,
}

impl Default for VerificationPolicy {
    fn default() -> Self {
        Self {
            phone_number: true,
            email: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AuthFactorsEnabled {
    pub sso: bool,
    pub email_password: bool,
    pub username_password: bool,
    pub email_otp: bool,
    pub email_magic_link: bool,
    pub phone_otp: bool,
    pub web3_wallet: bool,
    pub backup_code: bool,
    pub authenticator: bool,
    pub passkey: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EmailSettings {
    pub enabled: bool,
    pub required: bool,
    pub verify_signup: Option<bool>,
    pub otp_verification_allowed: Option<bool>,
    pub magic_link_verification_allowed: Option<bool>,
}

impl Default for EmailSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            required: true,
            verify_signup: Some(true),
            otp_verification_allowed: Some(true),
            magic_link_verification_allowed: Some(true),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PhoneSettings {
    pub enabled: bool,
    pub required: bool,
    pub verify_signup: Option<bool>,
    pub sms_verification_allowed: Option<bool>,
    pub whatsapp_verification_allowed: Option<bool>,
}

impl Default for PhoneSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            required: false,
            verify_signup: Some(true),
            sms_verification_allowed: Some(true),
            whatsapp_verification_allowed: Some(false),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UsernameSettings {
    pub enabled: bool,
    pub required: bool,
    pub min_length: Option<u8>,
    pub max_length: Option<u8>,
}

impl Default for UsernameSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            required: false,
            min_length: Some(3),
            max_length: Some(30),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EmailLinkSettings {
    pub enabled: bool,
    pub require_same_device: bool,
}

impl Default for EmailLinkSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            require_same_device: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PasskeySettings {
    pub enabled: bool,
    #[serde(default)]
    pub prompt_registration_on_auth: bool,
    pub allow_autofill: bool,
}

impl Default for PasskeySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            prompt_registration_on_auth: false,
            allow_autofill: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MultiSessionSupport {
    pub enabled: bool,
    pub max_accounts_per_session: i64,
    pub max_sessions_per_account: i64,
}

impl Default for MultiSessionSupport {
    fn default() -> Self {
        Self {
            enabled: true,
            max_accounts_per_session: 5,
            max_sessions_per_account: 5,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentAuthSettings {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub email_address: EmailSettings,
    pub phone_number: PhoneSettings,
    pub username: UsernameSettings,
    pub first_name: IndividualAuthSettings,
    pub last_name: IndividualAuthSettings,
    pub password: PasswordSettings,
    pub magic_link: Option<EmailLinkSettings>,
    pub passkey: Option<PasskeySettings>,
    pub auth_factors_enabled: AuthFactorsEnabled,
    pub verification_policy: VerificationPolicy,
    pub second_factor_policy: SecondFactorPolicy,
    pub first_factor: FirstFactor,
    pub multi_session_support: MultiSessionSupport,
    pub session_token_lifetime: i64,
    pub session_validity_period: i64,
    pub session_inactive_timeout: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
}

impl Default for DeploymentAuthSettings {
    fn default() -> Self {
        Self {
            id: 0,
            created_at: None,
            updated_at: None,
            email_address: EmailSettings::default(),
            phone_number: PhoneSettings::default(),
            username: UsernameSettings::default(),
            first_name: IndividualAuthSettings::default(),
            last_name: IndividualAuthSettings::default(),
            password: PasswordSettings::default(),
            magic_link: Some(EmailLinkSettings::default()),
            passkey: Some(PasskeySettings::default()),
            multi_session_support: MultiSessionSupport::default(),
            session_token_lifetime: 30 * 60,
            session_validity_period: 30 * 24 * 60 * 60,
            session_inactive_timeout: 7 * 60 * 60 * 24,
            auth_factors_enabled: AuthFactorsEnabled::default(),
            verification_policy: VerificationPolicy::default(),
            second_factor_policy: SecondFactorPolicy::Optional,
            first_factor: FirstFactor::EmailPassword,
            deployment_id: 0,
        }
    }
}

impl AuthFactorsEnabled {
    pub fn with_email(mut self, enabled: bool) -> Self {
        self.email_password = enabled;
        self.email_otp = enabled;
        self.email_magic_link = enabled;
        self
    }

    pub fn with_username(mut self, enabled: bool) -> Self {
        self.username_password = enabled;
        self
    }

    pub fn with_phone(mut self, enabled: bool) -> Self {
        self.phone_otp = enabled;
        self
    }
}
