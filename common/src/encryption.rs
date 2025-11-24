use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, prelude::BASE64_STANDARD};
use crate::error::AppError;

const NONCE_SIZE: usize = 12;

pub struct EncryptionService {
    cipher: Aes256Gcm,
}

impl EncryptionService {
    pub fn new(key_hex: &str) -> Result<Self, AppError> {
        let key_bytes = hex::decode(key_hex)
            .map_err(|e| AppError::Internal(format!("Invalid encryption key hex: {}", e)))?;

        if key_bytes.len() != 32 {
            return Err(AppError::Internal(
                "Encryption key must be 32 bytes (64 hex characters)".to_string(),
            ));
        }

        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);

        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, AppError> {
        let nonce_bytes: [u8; NONCE_SIZE] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| AppError::Internal(format!("Encryption failed: {}", e)))?;

        let mut combined = nonce_bytes.to_vec();
        combined.extend(ciphertext);

        Ok(BASE64_STANDARD.encode(&combined))
    }

    pub fn decrypt(&self, encrypted: &str) -> Result<String, AppError> {
        let combined = BASE64_STANDARD
            .decode(encrypted)
            .map_err(|e| AppError::Internal(format!("Invalid base64: {}", e)))?;

        if combined.len() < NONCE_SIZE {
            return Err(AppError::Internal("Invalid encrypted data".to_string()));
        }

        let (nonce_bytes, ciphertext) = combined.split_at(NONCE_SIZE);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| AppError::Internal(format!("Decryption failed: {}", e)))?;

        String::from_utf8(plaintext)
            .map_err(|e| AppError::Internal(format!("Invalid UTF-8: {}", e)))
    }
}

impl Clone for EncryptionService {
    fn clone(&self) -> Self {
        Self {
            cipher: self.cipher.clone(),
        }
    }
}
