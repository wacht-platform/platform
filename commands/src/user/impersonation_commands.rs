use common::error::AppError;
use josekit::jws::{ES256, JwsHeader};
use josekit::jwt::{self, JwtPayload};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ImpersonationTokenClaims {
    pub user_id: i64,
    pub deployment_id: i64,
    #[serde(rename = "type")]
    pub token_type: String,
}

pub struct GenerateImpersonationTokenCommand {
    deployment_id: i64,
    user_id: i64,
}

impl GenerateImpersonationTokenCommand {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'a, A>(
        self,
        acquirer: A,
    ) -> Result<GenerateImpersonationTokenResponse, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let keypair = sqlx::query!(
            r#"
            SELECT private_key, public_key, frontend_host
            FROM deployment_key_pairs dk
            JOIN deployments d ON d.id = dk.deployment_id
            WHERE dk.deployment_id = $1
            "#,
            self.deployment_id
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to get deployment keypair: {}", e)))?;

        let user = sqlx::query!(
            r#"
            SELECT id, disabled
            FROM users
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.user_id,
            self.deployment_id
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch user: {}", e)))?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

        if user.disabled {
            return Err(AppError::BadRequest(
                "Cannot impersonate disabled user".to_string(),
            ));
        }

        let mut payload = JwtPayload::new();
        payload.set_subject(&self.user_id.to_string());
        payload.set_issuer(&format!("https://{}", keypair.frontend_host));

        let now = std::time::SystemTime::now();
        let expires = now + std::time::Duration::from_secs(600);

        payload.set_issued_at(&now);
        payload.set_expires_at(&expires);

        payload
            .set_claim("user_id", Some(serde_json::json!(self.user_id.to_string())))
            .map_err(|e| AppError::Internal(format!("Failed to set user_id claim: {}", e)))?;
        payload
            .set_claim(
                "deployment_id",
                Some(serde_json::json!(self.deployment_id.to_string())),
            )
            .map_err(|e| AppError::Internal(format!("Failed to set deployment_id claim: {}", e)))?;
        payload
            .set_claim("type", Some(serde_json::json!("impersonation")))
            .map_err(|e| AppError::Internal(format!("Failed to set type claim: {}", e)))?;

        let signer = ES256
            .signer_from_pem(&keypair.private_key)
            .map_err(|e| AppError::Internal(format!("Failed to create signer: {}", e)))?;

        let mut header = JwsHeader::new();
        header.set_token_type("JWT");

        let token = jwt::encode_with_signer(&payload, &header, &signer)
            .map_err(|e| AppError::Internal(format!("Failed to encode JWT: {}", e)))?;

        let redirect_url = format!(
            "https://{}/sign-in?impersonation_token={}",
            keypair.frontend_host,
            urlencoding::encode(&token)
        );

        Ok(GenerateImpersonationTokenResponse {
            token,
            redirect_url,
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct GenerateImpersonationTokenResponse {
    pub token: String,
    pub redirect_url: String,
}
