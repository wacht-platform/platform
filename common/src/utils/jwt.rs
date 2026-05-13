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

/// Variant of `sign_token` that includes a `kid` header — required for OIDC
/// id_tokens so RPs can pick the right JWKS key without iterating.
pub fn sign_token_with_kid<T: Serialize>(
    claims: T,
    algorithm: &str,
    key: &str,
    kid: &str,
) -> Result<String, AppError> {
    let mut header = match algorithm {
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
    header.kid = Some(kid.to_string());

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
    pub sub: Option<String>,
    pub scope: Option<String>,
    pub aud: Option<String>,
    pub exp: Option<i64>,
    pub iat: Option<i64>,
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

/// Read the `kid` claim from a JWT header without verifying the signature.
/// Used by callers that need to pick the right verification key from a set
/// (OIDC id_token_hint, OAuth jwks-based client auth, …).
pub fn read_kid(token: &str) -> Result<Option<String>, AppError> {
    let header = jsonwebtoken::decode_header(token)
        .map_err(|e| AppError::BadRequest(format!("malformed JWT: {}", e)))?;
    Ok(header.kid)
}

/// Verify a JWT with explicit issuer/audience checks and optional expiry
/// enforcement. The general `verify_token` helper forces `validate_exp=true`
/// and skips audience, which doesn't fit every protocol-level flow.
///
/// OIDC RP-Initiated Logout (`id_token_hint`) is the motivating case: the spec
/// says the OP MUST accept previously-issued id_tokens — including expired
/// ones — as long as `iss` matches and (when supplied) `aud` matches the
/// client_id the RP claims to be acting on behalf of.
pub fn verify_token_with_claims<T: for<'de> Deserialize<'de>>(
    token: &str,
    algorithm: &str,
    key: &str,
    expected_issuer: &str,
    expected_audience: Option<&str>,
    validate_exp: bool,
) -> Result<TokenData<T>, AppError> {
    let alg = match algorithm {
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

    let decoding_key = match alg {
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

    let mut validation = Validation::new(alg);
    validation.validate_exp = validate_exp;
    validation.set_issuer(&[expected_issuer]);
    if let Some(aud) = expected_audience {
        validation.set_audience(&[aud]);
    } else {
        validation.validate_aud = false;
    }
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
