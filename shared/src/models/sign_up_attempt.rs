use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SignupAttemptStep {
    VerifyEmail,
    VerifyPhone,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SignupAttemptStatus {
    Pending,
    Complete,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SignupAttempt {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub session_id: i64,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub username: String,
    pub phone_number: String,
    pub password: String,
    pub required_fields: Vec<String>,
    pub missing_fields: Vec<String>,
    pub current_step: SignupAttemptStep,
    pub remaining_steps: Vec<SignupAttemptStep>,
}
