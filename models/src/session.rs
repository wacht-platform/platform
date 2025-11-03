use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SignInAttemptStep {
    VerifyEmail,
    VerifyEmailOtp,
    VerifySecondFactor,
    VerifyPhone,
    VerifyPhoneOtp,
    PasswordResetInitiation,
    PasswordResetCompletion,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub active_signin_id: Option<i64>,
}
