use common::error::AppError;
use josekit::jws::{ES256, JwsHeader};
use josekit::jwt::{self, JwtPayload};
use sqlx::Row;

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

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<GenerateImpersonationTokenResponse, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT dk.private_key, d.frontend_host, u.disabled
            FROM deployment_key_pairs dk
            JOIN deployments d ON d.id = dk.deployment_id
            JOIN users u ON u.deployment_id = dk.deployment_id
            WHERE dk.deployment_id = $1
              AND u.id = $2
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.user_id)
        .fetch_optional(executor)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch impersonation context: {}", e)))?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

        let disabled: bool = row.get("disabled");
        if disabled {
            return Err(AppError::BadRequest(
                "Cannot impersonate disabled user".to_string(),
            ));
        }

        let private_key: String = row.get("private_key");
        let frontend_host: String = row.get("frontend_host");

        let mut payload = JwtPayload::new();
        payload.set_subject(&self.user_id.to_string());
        payload.set_issuer(&format!("https://{}", frontend_host));

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
            .signer_from_pem(&private_key)
            .map_err(|e| AppError::Internal(format!("Failed to create signer: {}", e)))?;

        let mut header = JwsHeader::new();
        header.set_token_type("JWT");

        let token = jwt::encode_with_signer(&payload, &header, &signer)
            .map_err(|e| AppError::Internal(format!("Failed to encode JWT: {}", e)))?;

        let redirect_url = format!(
            "https://{}/sign-in?impersonation_token={}",
            frontend_host,
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
