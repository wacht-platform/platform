use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    pub config: McpServerConfig,
}

pub fn slugify_mcp_server_name(name: &str) -> String {
    let lowered: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let mut out = String::with_capacity(lowered.len());
    let mut prev_underscore = false;
    for c in lowered.chars() {
        if c == '_' {
            if !prev_underscore && !out.is_empty() {
                out.push('_');
                prev_underscore = true;
            }
        } else {
            out.push(c);
            prev_underscore = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<McpAuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConnectionMetadata {
    pub auth_type: String,
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connected_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpAuthConfig {
    #[serde(rename = "token")]
    Token { auth_token: String },
    #[serde(rename = "oauth_client_credentials")]
    OAuthClientCredentials {
        client_id: String,
        client_secret: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_url: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scopes: Vec<String>,
    },
    #[serde(rename = "oauth_authorization_code_public_pkce")]
    OAuthAuthorizationCodePublicPkce {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        register_url: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scopes: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resource: Option<String>,
    },
    #[serde(rename = "oauth_authorization_code_confidential_pkce")]
    OAuthAuthorizationCodeConfidentialPkce {
        client_id: String,
        client_secret: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_url: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scopes: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resource: Option<String>,
    },
}

impl McpAuthConfig {
    pub fn requires_user_connection(&self) -> bool {
        matches!(
            self,
            Self::OAuthAuthorizationCodePublicPkce { .. }
                | Self::OAuthAuthorizationCodeConfidentialPkce { .. }
        )
    }
}
