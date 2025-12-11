use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentKeyPair {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deployment_id: i64,
    pub public_key: String,
    pub private_key: String,
    pub saml_public_key: Option<String>,
    pub saml_private_key: Option<String>,
}
