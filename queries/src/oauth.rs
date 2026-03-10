use chrono::{DateTime, Utc};
use common::error::AppError;
use common::json_utils::json_default;
use models::api_key::{JwksDocument, OAuthScopeDefinition};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthAppData {
    pub id: i64,
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub fqdn: String,
    pub supported_scopes: serde_json::Value,
    pub scope_definitions: serde_json::Value,
    pub allow_dynamic_client_registration: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthAppData {
    pub fn supported_scopes_vec(&self) -> Vec<String> {
        json_default(self.supported_scopes.clone())
    }

    pub fn scope_definitions_vec(&self) -> Vec<OAuthScopeDefinition> {
        json_default(self.scope_definitions.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientData {
    pub id: i64,
    pub deployment_id: i64,
    pub oauth_app_id: i64,
    pub client_id: String,
    pub client_auth_method: String,
    pub grant_types: serde_json::Value,
    pub redirect_uris: serde_json::Value,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: serde_json::Value,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub pkce_required: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthClientData {
    pub fn grant_types_vec(&self) -> Vec<String> {
        json_default(self.grant_types.clone())
    }

    pub fn redirect_uris_vec(&self) -> Vec<String> {
        json_default(self.redirect_uris.clone())
    }

    pub fn contacts_vec(&self) -> Vec<String> {
        json_default(self.contacts.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientGrantData {
    pub id: i64,
    pub deployment_id: i64,
    pub api_auth_app_slug: String,
    pub oauth_client_id: i64,
    pub resource: String,
    pub scopes: serde_json::Value,
    pub status: String,
    pub granted_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub granted_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthClientGrantData {
    pub fn scopes_vec(&self) -> Vec<String> {
        json_default(self.scopes.clone())
    }
}

mod apps;
mod clients;
mod grants;

pub use apps::*;
pub use clients::*;
pub use grants::*;
