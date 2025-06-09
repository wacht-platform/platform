use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{SecondFactorPolicy, SchemaVersion, SocialConnection, UserEmailAddress, UserPhoneNumber};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_name: String,
    pub last_name: String,
    pub username: Option<String>,
    pub profile_picture_url: String,
    pub schema_version: SchemaVersion,
    pub disabled: bool,
    pub second_factor_policy: SecondFactorPolicy,
    pub active_organization_membership_id: Option<i64>,
    pub active_workspace_membership_id: Option<i64>,
    pub deployment_id: i64,
    pub public_metadata: Value,
    pub private_metadata: Value,

    // Primary identifiers
    pub primary_email_address: Option<String>,
    pub primary_phone_number: Option<String>,

    // All identifiers
    pub email_addresses: Vec<UserEmailAddress>,
    pub phone_numbers: Vec<UserPhoneNumber>,
    pub social_connections: Vec<SocialConnection>,

    // Authentication
    pub has_password: bool,
    pub has_otp: bool,
    pub has_backup_codes: bool,
}
