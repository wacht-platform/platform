use argon2::{
    Argon2,
    password_hash::{
        PasswordHash, PasswordHasher as Argon2PasswordHasher, PasswordVerifier, SaltString,
        rand_core::{OsRng, RngCore},
    },
};
use totp_rs::TOTP;

use crate::error::AppError;

pub struct PasswordHasher;

impl PasswordHasher {
    pub fn hash_password(password: &str) -> Result<String, AppError> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| AppError::BadRequest(format!("Failed to hash password: {}", e)))?;

        Ok(password_hash.to_string())
    }

    pub fn _verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| AppError::BadRequest(format!("Invalid password hash: {}", e)))?;

        let argon2 = Argon2::default();

        match argon2.verify_password(password.as_bytes(), &parsed_hash) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

pub struct TotpGenerator;

impl TotpGenerator {
    pub fn generate_secret() -> Result<String, AppError> {
        let mut secret_bytes = [0u8; 20];
        OsRng.fill_bytes(&mut secret_bytes);

        let totp = TOTP::new(totp_rs::Algorithm::SHA1, 6, 1, 30, secret_bytes.to_vec())
            .map_err(|e| AppError::BadRequest(format!("Failed to generate TOTP: {}", e)))?;

        Ok(totp.get_secret_base32())
    }
}


