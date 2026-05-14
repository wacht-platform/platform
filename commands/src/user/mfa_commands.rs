use common::encryption::EncryptionService;
use common::error::AppError;
use common::utils::security::PasswordHasher;
use rand::RngCore;
use totp_rs::Secret;

const BACKUP_CODE_COUNT: usize = 12;
const BACKUP_CODE_CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz";

fn generate_backup_codes() -> Vec<String> {
    let mut codes = Vec::with_capacity(BACKUP_CODE_COUNT);
    let mut rng = rand::rng();
    let mut buf = [0u8; 8];
    for _ in 0..BACKUP_CODE_COUNT {
        rng.fill_bytes(&mut buf);
        let chars: String = buf
            .iter()
            .map(|b| BACKUP_CODE_CHARSET[(*b as usize) % BACKUP_CODE_CHARSET.len()] as char)
            .collect();
        codes.push(format!("{}-{}", &chars[..4], &chars[4..8]));
    }
    codes
}

fn hash_backup_codes(codes: &[String]) -> Result<Vec<String>, AppError> {
    codes
        .iter()
        .map(|c| PasswordHasher::hash_password(c))
        .collect()
}

pub struct DeleteUserAuthenticatorCommand {
    deployment_id: i64,
    user_id: i64,
}

impl DeleteUserAuthenticatorCommand {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            UPDATE user_authenticators
            SET deleted_at = NOW(), updated_at = NOW()
            FROM users u
            WHERE user_authenticators.user_id = u.id
              AND u.deployment_id = $1
              AND user_authenticators.user_id = $2
              AND user_authenticators.deleted_at IS NULL
            "#,
            self.deployment_id,
            self.user_id
        )
        .execute(executor)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("authenticator not found".to_string()));
        }
        Ok(())
    }
}

pub struct CreateUserAuthenticatorResponse {
    pub id: i64,
    pub otp_url: String,
}

pub struct CreateUserAuthenticatorCommand {
    deployment_id: i64,
    user_id: i64,
    authenticator_id: i64,
    /// Base32-encoded TOTP secret supplied by the caller. Validated and
    /// normalized (whitespace stripped, uppercased) before storage.
    secret_base32: String,
    /// Optional label shown in the user's authenticator app. If absent, the
    /// server derives one from the user's primary email / username / name.
    account_name_override: Option<String>,
}

impl CreateUserAuthenticatorCommand {
    pub fn new(
        deployment_id: i64,
        user_id: i64,
        authenticator_id: i64,
        secret_base32: String,
    ) -> Self {
        Self {
            deployment_id,
            user_id,
            authenticator_id,
            secret_base32,
            account_name_override: None,
        }
    }

    pub fn with_account_name(mut self, account_name: Option<String>) -> Self {
        self.account_name_override = account_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        self
    }

    pub async fn execute_with_pool(
        self,
        pool: &sqlx::PgPool,
        enc: &EncryptionService,
    ) -> Result<CreateUserAuthenticatorResponse, AppError> {
        // Normalize and validate the supplied secret. Strip whitespace (people
        // paste secrets with spaces every 4 chars), uppercase, then run it
        // through Secret::Encoded which validates the base32 alphabet AND
        // length is decodable.
        let normalized: String = self
            .secret_base32
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect::<String>()
            .to_ascii_uppercase();

        let decoded = Secret::Encoded(normalized.clone())
            .to_bytes()
            .map_err(|_| AppError::BadRequest("invalid base32 secret".to_string()))?;
        if decoded.len() < 16 {
            return Err(AppError::BadRequest(
                "secret too short — TOTP secrets must be at least 128 bits (16 bytes)"
                    .to_string(),
            ));
        }

        let mut tx = pool.begin().await?;

        // Lock the user row so a concurrent admin call can't race the
        // existence check below.
        let row = sqlx::query!(
            r#"
            SELECT
                COALESCE(NULLIF(dus.app_name, ''), 'Wacht') AS "issuer!",
                COALESCE(
                    NULLIF(e.email_address, ''),
                    NULLIF(u.username, ''),
                    NULLIF(trim(both ' ' from u.first_name || ' ' || u.last_name), ''),
                    'user-' || u.id::text
                ) AS "account!"
            FROM users u
            LEFT JOIN deployment_ui_settings dus
              ON dus.deployment_id = u.deployment_id
            LEFT JOIN user_email_addresses e
              ON e.id = u.primary_email_address_id
            WHERE u.deployment_id = $1 AND u.id = $2
            FOR UPDATE OF u
            "#,
            self.deployment_id,
            self.user_id,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let row = row.ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

        let existing = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM user_authenticators WHERE user_id = $1 AND deleted_at IS NULL) AS \"exists!\"",
            self.user_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if existing {
            return Err(AppError::Conflict(
                "user already has an active authenticator".to_string(),
            ));
        }

        // Reject `:` in the account label — it's the otpauth path separator
        // and would produce an invalid URL.
        if let Some(name) = &self.account_name_override {
            if name.contains(':') {
                return Err(AppError::BadRequest(
                    "account_name cannot contain ':'".to_string(),
                ));
            }
        }

        // Format the otpauth URL ourselves — totp-rs's `get_url()` lives behind
        // the `otpauth` feature flag, not enabled in our build.
        let account = self.account_name_override.as_deref().unwrap_or(&row.account);
        let issuer_enc = urlencoding::encode(&row.issuer);
        let account_enc = urlencoding::encode(account);
        let otp_url = format!(
            "otpauth://totp/{issuer}:{account}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits=6&period=30",
            issuer = issuer_enc,
            account = account_enc,
            secret = normalized,
        );

        let encrypted_secret = enc.encrypt(&normalized)?;

        sqlx::query!(
            r#"
            INSERT INTO user_authenticators (id, created_at, updated_at, user_id, totp_secret, otp_url)
            VALUES ($1, NOW(), NOW(), $2, $3, $4)
            "#,
            self.authenticator_id,
            self.user_id,
            encrypted_secret,
            otp_url,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(CreateUserAuthenticatorResponse {
            id: self.authenticator_id,
            otp_url,
        })
    }
}

pub struct RegenerateUserBackupCodesCommand {
    deployment_id: i64,
    user_id: i64,
}

impl RegenerateUserBackupCodesCommand {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<Vec<String>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let codes = generate_backup_codes();
        let hashed = hash_backup_codes(&codes)?;
        let result = sqlx::query!(
            r#"
            UPDATE users
            SET backup_codes = $1, backup_codes_generated = true, updated_at = NOW()
            WHERE deployment_id = $2 AND id = $3
            "#,
            &hashed,
            self.deployment_id,
            self.user_id
        )
        .execute(executor)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("user not found".to_string()));
        }
        // Return the plaintext codes — this is the user's only chance to see them.
        Ok(codes)
    }
}
