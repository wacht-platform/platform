use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct OrganizationInvitation {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub organization_id: i64,
    pub email: String,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub initial_organization_role_id: Option<i64>,
    pub initial_organization_role_name: Option<String>,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub inviter_id: Option<i64>,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub workspace_id: Option<i64>,
    pub workspace_name: Option<String>,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub initial_workspace_role_id: Option<i64>,
    pub initial_workspace_role_name: Option<String>,
    pub expired: bool,
    pub expiry: Option<DateTime<Utc>>,
    /// Random token used to construct the accept-invitation URL. Returned so
    /// admin tooling can render the link when out-of-band sharing is needed.
    pub token: String,
}
