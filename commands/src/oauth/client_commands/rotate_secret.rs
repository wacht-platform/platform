use super::*;
pub struct RotateOAuthClientSecret {
    pub oauth_app_id: i64,
    pub client_id: String,
}

impl RotateOAuthClientSecret {
    fn generate_client_secret_hash_and_encrypted(
        encryptor: &dyn OAuthClientSecretEncryptor,
    ) -> Result<(String, String, String), AppError> {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use rand::RngCore;
        let mut random_bytes = vec![0u8; 32];
        rand::rng().fill_bytes(&mut random_bytes);
        let secret = format!("oc_secret_{}", URL_SAFE_NO_PAD.encode(random_bytes));

        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        let encrypted = encryptor.encrypt(&secret)?;
        Ok((secret, hash, encrypted))
    }
}

impl RotateOAuthClientSecret {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Option<String>, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider,
    {
        let writer = deps.db_router().writer();
        let encryptor = deps.encryption_provider();
        let client = sqlx::query!(
            r#"
            SELECT client_auth_method
            FROM oauth_clients
            WHERE oauth_app_id = $1
              AND client_id = $2
              AND is_active = TRUE
            "#,
            self.oauth_app_id,
            self.client_id
        )
        .fetch_optional(writer)
        .await?;

        let Some(client) = client else {
            return Ok(None);
        };

        if client.client_auth_method == "none" || client.client_auth_method == "private_key_jwt" {
            return Err(AppError::Validation(
                "client_secret rotation is not supported for this auth method".to_string(),
            ));
        }

        let (client_secret, client_secret_hash, client_secret_encrypted) =
            Self::generate_client_secret_hash_and_encrypted(encryptor)?;
        sqlx::query!(
            r#"
            UPDATE oauth_clients
            SET
                client_secret_hash = $3,
                client_secret_encrypted = $4,
                updated_at = NOW()
            WHERE oauth_app_id = $1
              AND client_id = $2
              AND is_active = TRUE
            "#,
            self.oauth_app_id,
            self.client_id,
            client_secret_hash,
            client_secret_encrypted
        )
        .execute(writer)
        .await?;

        Ok(Some(client_secret))
    }
}
