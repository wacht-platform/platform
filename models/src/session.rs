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
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub active_signin_id: Option<i64>,
    /// Tenant scope. Nullable until the Pass-2 NOT NULL migration ships; new
    /// rows are written with a value. Always validate this matches the
    /// requesting deployment before consuming a session.
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub deployment_id: Option<i64>,
    /// Soft-delete / logout marker. Use `deleted_at IS NULL` to detect an
    /// active session; flip to NOW() to revoke (logout, MFA failure, admin
    /// kick) — refresh-token / token-mint paths must check this.
    pub deleted_at: Option<DateTime<Utc>>,
}
