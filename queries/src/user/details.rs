use super::*;

pub struct GetUserDetailsQuery {
    deployment_id: i64,
    user_id: i64,
}

pub struct GetVerifiedEmailTemplateUserQuery {
    deployment_id: i64,
    email: String,
}

impl GetUserDetailsQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<UserDetails, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let user_row = sqlx::query!(
            r#"
            SELECT
                u.id, u.created_at, u.updated_at,
                u.first_name, u.last_name, u.username, u.profile_picture_url,
                u.schema_version, u.disabled, u.second_factor_policy,
                u.availability, u.last_password_reset_at,
                u.active_organization_membership_id, u.active_workspace_membership_id,
                u.primary_email_address_id, u.primary_phone_number_id,
                u.deployment_id, u.public_metadata, u.private_metadata,
                u.password, u.backup_codes,
                e.email_address as primary_email_address,
                p.phone_number as "primary_phone_number?"
            FROM users u
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            WHERE u.deployment_id = $1 AND u.id = $2
            "#,
            self.deployment_id,
            self.user_id
        )
        .fetch_one(&mut *conn)
        .await?;

        let email_rows = sqlx::query!(
            r#"
            SELECT
                id, created_at, updated_at, deployment_id, user_id,
                email_address as email, is_primary, verified, verified_at,
                verification_strategy
            FROM user_email_addresses
            WHERE user_id = $1
            "#,
            self.user_id
        )
        .fetch_all(&mut *conn)
        .await?;

        let email_addresses = email_rows
            .into_iter()
            .map(|row| -> Result<UserEmailAddress, AppError> {
                let verification_strategy = match row.verification_strategy {
                    Some(s) => models::VerificationStrategy::from_str(&s).map_err(|_| {
                        AppError::Internal(format!("Invalid verification_strategy: {}", s))
                    })?,
                    None => models::VerificationStrategy::Otp,
                };

                Ok(UserEmailAddress {
                    id: row.id,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    deployment_id: row.deployment_id.unwrap_or(self.deployment_id),
                    user_id: row.user_id.unwrap_or(self.user_id),
                    email: row.email.unwrap_or_default(),
                    is_primary: row.is_primary,
                    verified: row.verified,
                    verified_at: row.verified_at.unwrap_or_else(chrono::Utc::now),
                    verification_strategy,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let phone_rows = sqlx::query!(
            r#"
            SELECT
                id, created_at, updated_at, user_id,
                phone_number, country_code, verified, verified_at
            FROM user_phone_numbers
            WHERE user_id = $1
            "#,
            self.user_id
        )
        .fetch_all(&mut *conn)
        .await?;

        let phone_numbers = phone_rows
            .into_iter()
            .map(|row| UserPhoneNumber {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                user_id: row.user_id.unwrap_or(self.user_id),
                phone_number: row.phone_number,
                country_code: row.country_code,
                verified: row.verified,
                verified_at: row.verified_at.unwrap_or_else(chrono::Utc::now),
            })
            .collect();

        let social_rows = sqlx::query!(
            r#"
            SELECT
                id, created_at, updated_at, user_id, user_email_address_id,
                provider, email_address, access_token, refresh_token
            FROM social_connections
            WHERE user_id = $1
            "#,
            self.user_id
        )
        .fetch_all(&mut *conn)
        .await?;

        let social_connections = social_rows
            .into_iter()
            .map(|row| SocialConnection {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                user_id: row.user_id,
                user_email_address_id: row.user_email_address_id,
                provider: models::SocialConnectionProvider::from_str(&row.provider)
                    .unwrap_or(models::SocialConnectionProvider::GoogleOauth),
                email_address: row.email_address,
                access_token: row.access_token.unwrap_or_default(),
                refresh_token: row.refresh_token.unwrap_or_default(),
            })
            .collect();

        let segments = sqlx::query_as!(
            models::Segment,
            r#"
            SELECT s.id, s.created_at, s.updated_at, s.deleted_at, s.deployment_id, s.name,
                   s.type as "segment_type: _"
            FROM segments s
            JOIN user_segments us ON s.id = us.segment_id
            WHERE us.user_id = $1
            "#,
            self.user_id
        )
        .fetch_all(&mut *conn)
        .await?;

        let user_details = UserDetails {
            id: user_row.id,
            created_at: user_row.created_at,
            updated_at: user_row.updated_at,
            first_name: user_row.first_name,
            last_name: user_row.last_name,
            username: if user_row.username.is_empty() {
                None
            } else {
                Some(user_row.username)
            },
            profile_picture_url: user_row.profile_picture_url,
            schema_version: models::SchemaVersion::from_str(&user_row.schema_version)
                .unwrap_or(models::SchemaVersion::V1),
            disabled: user_row.disabled,
            second_factor_policy: models::SecondFactorPolicy::from_str(
                &user_row.second_factor_policy,
            )
            .unwrap_or(models::SecondFactorPolicy::Optional),
            availability: user_row.availability,
            last_password_reset_at: user_row.last_password_reset_at,
            active_organization_membership_id: user_row.active_organization_membership_id,
            active_workspace_membership_id: user_row.active_workspace_membership_id,
            deployment_id: user_row.deployment_id,
            public_metadata: user_row.public_metadata,
            private_metadata: user_row.private_metadata,
            primary_email_address: user_row.primary_email_address,
            primary_phone_number: user_row.primary_phone_number,
            primary_email_address_id: user_row.primary_email_address_id.map(|id| id.to_string()),
            primary_phone_number_id: user_row.primary_phone_number_id.map(|id| id.to_string()),
            email_addresses,
            phone_numbers,
            social_connections,
            segments,
            has_password: user_row.password.is_some()
                && !user_row.password.unwrap_or_default().is_empty(),
            has_backup_codes: user_row.backup_codes.is_some()
                && !user_row.backup_codes.unwrap_or_default().is_empty(),
        };
        Ok(user_details)
    }
}

impl GetVerifiedEmailTemplateUserQuery {
    pub fn new(deployment_id: i64, email: impl Into<String>) -> Self {
        Self {
            deployment_id,
            email: email.into(),
        }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Option<serde_json::Value>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query!(
            r#"
            SELECT
                u.id,
                u.first_name,
                u.last_name,
                u.username,
                u.profile_picture_url,
                u.created_at,
                u.disabled,
                u.password,
                u.public_metadata,
                u.private_metadata,
                primary_email.email_address AS primary_email_address,
                primary_phone.phone_number AS "primary_phone_number?"
            FROM user_email_addresses target_email
            JOIN users u
              ON u.id = target_email.user_id
             AND u.deployment_id = target_email.deployment_id
            LEFT JOIN user_email_addresses primary_email
              ON primary_email.id = u.primary_email_address_id
            LEFT JOIN user_phone_numbers primary_phone
              ON primary_phone.id = u.primary_phone_number_id
            WHERE target_email.deployment_id = $1
              AND lower(target_email.email_address) = lower($2)
              AND target_email.verified = true
              AND target_email.user_id IS NOT NULL
            ORDER BY target_email.is_primary DESC,
                     target_email.verified_at DESC NULLS LAST,
                     target_email.id DESC
            LIMIT 1
            "#,
            self.deployment_id,
            self.email
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(row.map(|row| {
            serde_json::json!({
                "id": row.id.to_string(),
                "first_name": row.first_name,
                "last_name": row.last_name,
                "full_name": format!("{} {}", row.first_name, row.last_name),
                "username": if row.username.is_empty() {
                    None::<String>
                } else {
                    Some(row.username)
                },
                "primary_email": row.primary_email_address,
                "primary_phone": row.primary_phone_number,
                "profile_picture_url": row.profile_picture_url,
                "created_at": row.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                "disabled": row.disabled,
                "has_password": row.password.is_some() && !row.password.unwrap_or_default().is_empty()
            })
        }))
    }
}
