use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;

use crate::error::AppError;

pub fn sign_token<T: Serialize>(claims: T, algorithm: &str, key: &str) -> Result<String, AppError> {
    let header = match algorithm {
        "HS256" => Header::new(Algorithm::HS256),
        "HS384" => Header::new(Algorithm::HS384),
        "HS512" => Header::new(Algorithm::HS512),
        "RS256" => Header::new(Algorithm::RS256),
        "RS384" => Header::new(Algorithm::RS384),
        "RS512" => Header::new(Algorithm::RS512),
        "ES256" => Header::new(Algorithm::ES256),
        "ES384" => Header::new(Algorithm::ES384),
        _ => {
            return Err(AppError::BadRequest(format!(
                "Unsupported algorithm: {}",
                algorithm
            )));
        }
    };

    let encoding_key = match algorithm {
        "HS256" | "HS384" | "HS512" => EncodingKey::from_secret(key.as_bytes()),
        "RS256" | "RS384" | "RS512" => EncodingKey::from_rsa_pem(key.as_bytes())
            .map_err(|_| AppError::Internal("Invalid RSA key".to_string()))?,
        "ES256" | "ES384" => EncodingKey::from_ec_pem(key.as_bytes())
            .map_err(|_| AppError::Internal("Invalid EC key".to_string()))?,
        _ => {
            return Err(AppError::BadRequest("Unsupported algorithm".to_string()));
        }
    };

    encode(&header, &claims, &encoding_key)
        .map_err(|_| AppError::Internal("Failed to sign token".to_string()))
}