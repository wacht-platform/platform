use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation, decode, encode,
};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Deserialize)]
pub struct AgentContextClaims {
    pub sub: Option<String>,   // Subject (user_id)
    pub scope: Option<String>, // Scope should contain "agent_context"
    pub aud: Option<String>,   // Audience - intended context group/resource
    pub exp: Option<i64>,      // Expiration time
    pub iat: Option<i64>,      // Issued at
}

pub fn verify_token<T: for<'de> Deserialize<'de>>(
    token: &str,
    algorithm: &str,
    key: &str,
) -> Result<TokenData<T>, AppError> {
    let algorithm = match algorithm {
        "HS256" => Algorithm::HS256,
        "HS384" => Algorithm::HS384,
        "HS512" => Algorithm::HS512,
        "RS256" => Algorithm::RS256,
        "RS384" => Algorithm::RS384,
        "RS512" => Algorithm::RS512,
        "ES256" => Algorithm::ES256,
        "ES384" => Algorithm::ES384,
        _ => {
            return Err(AppError::BadRequest(format!(
                "Unsupported algorithm: {}",
                algorithm
            )));
        }
    };

    let decoding_key = match algorithm {
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            DecodingKey::from_secret(key.as_bytes())
        }
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
            DecodingKey::from_rsa_pem(key.as_bytes())
                .map_err(|_| AppError::Internal("Invalid RSA key".to_string()))?
        }
        Algorithm::ES256 | Algorithm::ES384 => DecodingKey::from_ec_pem(key.as_bytes())
            .map_err(|_| AppError::Internal("Invalid EC key".to_string()))?,
        _ => {
            return Err(AppError::BadRequest("Unsupported algorithm".to_string()));
        }
    };

    let mut validation = Validation::new(algorithm);
    validation.validate_exp = true;
    validation.validate_aud = false;

    decode::<T>(token, &decoding_key, &validation)
        .map_err(|e| AppError::BadRequest(format!("Invalid token: {}", e)))
}

pub fn verify_agent_context_token(
    token: &str,
    algorithm: &str,
    key: &str,
    expected_subject: Option<&str>,
) -> Result<AgentContextClaims, AppError> {
    let token_data = verify_token::<AgentContextClaims>(token, algorithm, key)?;
    let claims = token_data.claims;

    // Check if scope contains "agent_context"
    if let Some(scope) = &claims.scope {
        if !scope.contains("agent_context") {
            return Err(AppError::BadRequest(
                "Token does not have agent_context scope".to_string(),
            ));
        }
    } else {
        return Err(AppError::BadRequest("Token missing scope".to_string()));
    }

    // Check subject if provided
    if let Some(expected_sub) = expected_subject {
        if let Some(sub) = &claims.sub {
            if sub != expected_sub {
                return Err(AppError::BadRequest(
                    "Token subject does not match expected subject".to_string(),
                ));
            }
        } else {
            return Err(AppError::BadRequest("Token missing subject".to_string()));
        }
    }

    Ok(claims)
}
