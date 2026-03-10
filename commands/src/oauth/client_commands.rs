use common::{EncryptionService, HasDbRouter, HasEncryptionProvider, error::AppError};
use common::json_utils::json_default;
use models::api_key::JwksDocument;
use queries::oauth::OAuthClientData;
use sha2::{Digest, Sha256};

pub trait OAuthClientSecretEncryptor: Send + Sync {
    fn encrypt(&self, plaintext: &str) -> Result<String, AppError>;
}

impl OAuthClientSecretEncryptor for EncryptionService {
    fn encrypt(&self, plaintext: &str) -> Result<String, AppError> {
        EncryptionService::encrypt(self, plaintext)
    }
}

fn validate_jwks_document(jwks: &JwksDocument) -> Result<(), AppError> {
    if jwks.keys.is_empty() {
        return Err(AppError::Validation(
            "jwks.keys must include at least one key".to_string(),
        ));
    }

    for key in &jwks.keys {
        if key.k.is_some() {
            return Err(AppError::Validation(
                "jwks for private_key_jwt must contain public keys, not symmetric secrets"
                    .to_string(),
            ));
        }
    }

    Ok(())
}

fn validate_optional_url(field_name: &str, value: Option<&str>) -> Result<(), AppError> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(());
    };

    let parsed = url::Url::parse(value)
        .map_err(|_| AppError::Validation(format!("{field_name} must be a valid URL")))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(AppError::Validation(format!(
            "{field_name} must use http or https"
        )));
    }

    Ok(())
}

fn validate_redirect_uris(redirect_uris: &[String], grant_types: &[String]) -> Result<(), AppError> {
    let requires_redirect = grant_types.iter().any(|g| g == "authorization_code");
    if requires_redirect && redirect_uris.is_empty() {
        return Err(AppError::Validation(
            "redirect_uris must include at least one URI for authorization_code".to_string(),
        ));
    }

    for uri in redirect_uris {
        let trimmed = uri.trim();
        if trimmed.is_empty() {
            return Err(AppError::Validation(
                "redirect_uris must not contain empty values".to_string(),
            ));
        }
        let parsed = url::Url::parse(trimmed).map_err(|_| {
            AppError::Validation("redirect_uris must contain valid URLs".to_string())
        })?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(AppError::Validation(
                "redirect_uris must use http or https".to_string(),
            ));
        }
        if parsed.fragment().is_some() {
            return Err(AppError::Validation(
                "redirect_uris must not include fragments".to_string(),
            ));
        }
    }

    Ok(())
}

fn validate_oauth_client_grant_types(grant_types: &[String]) -> Result<(), AppError> {
    if grant_types.is_empty() {
        return Err(AppError::Validation(
            "grant_types must include at least one grant".to_string(),
        ));
    }

    let mut has_authorization_code = false;

    for grant in grant_types {
        let value = grant.trim();
        if value.is_empty() {
            return Err(AppError::Validation(
                "grant_types must not contain empty values".to_string(),
            ));
        }

        match value {
            "authorization_code" => {
                has_authorization_code = true;
            }
            "refresh_token" => {}
            "client_credentials" => {
                return Err(AppError::Validation(
                    "client_credentials is disabled for now".to_string(),
                ));
            }
            _ => {
                return Err(AppError::Validation(format!(
                    "unsupported grant_type: {}. supported values: authorization_code, refresh_token",
                    value
                )));
            }
        }
    }

    if !has_authorization_code {
        return Err(AppError::Validation(
            "authorization_code grant is required".to_string(),
        ));
    }

    Ok(())
}

mod create_client;
mod lifecycle;
mod rotate_secret;
mod update_settings;

pub use create_client::*;
pub use lifecycle::*;
pub use rotate_secret::*;
pub use update_settings::*;
