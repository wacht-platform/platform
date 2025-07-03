use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::SignInAttemptStep;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SignInMethod {
    PlainEmail,
    PlainUsername,
    PhoneOtp,
    MagicLink,
    EmailOtp,
    Sso,
    Passkey,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Error {
    pub code: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SignInAttempt {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub user_id: i64,
    pub identifier_id: i64,
    pub session_id: i64,
    pub method: SignInMethod,
    pub sso_provider: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub current_step: SignInAttemptStep,
    pub remaining_steps: Vec<SignInAttemptStep>,
    pub completed: bool,
    pub errored: bool,
    pub errors: Vec<Error>,
}
