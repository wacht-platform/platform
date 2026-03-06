use common::error::AppError;

pub struct RevokeOAuthRefreshTokenById {
    pub refresh_token_id: i64,
}

impl RevokeOAuthRefreshTokenById {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let res = sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET revoked_at = NOW()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.refresh_token_id)
        .execute(&mut *conn)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}

pub struct SetOAuthRefreshTokenReplacement {
    pub old_refresh_token_id: i64,
    pub new_refresh_token_id: i64,
}

impl SetOAuthRefreshTokenReplacement {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET replaced_by_token_id = $2
            WHERE id = $1
            "#,
        )
        .bind(self.old_refresh_token_id)
        .bind(self.new_refresh_token_id)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}

pub struct RevokeOAuthAccessTokenByHash {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub token_hash: String,
}

impl RevokeOAuthAccessTokenByHash {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query(
            r#"
            UPDATE oauth_access_tokens
            SET revoked_at = NOW()
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND token_hash = $3
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.token_hash)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}

pub struct RevokeOAuthRefreshTokenByHash {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub token_hash: String,
}

impl RevokeOAuthRefreshTokenByHash {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET revoked_at = NOW()
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND token_hash = $3
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.token_hash)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}

pub struct RevokeOAuthRefreshTokenFamily {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub root_refresh_token_id: i64,
}

impl RevokeOAuthRefreshTokenFamily {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<u64, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let res = sqlx::query(
            r#"
            WITH RECURSIVE token_chain AS (
                SELECT id, replaced_by_token_id
                FROM oauth_refresh_tokens
                WHERE deployment_id = $1
                  AND oauth_client_id = $2
                  AND id = $3
                UNION ALL
                SELECT rt.id, rt.replaced_by_token_id
                FROM oauth_refresh_tokens rt
                INNER JOIN token_chain chain
                    ON rt.id = chain.replaced_by_token_id
                WHERE rt.deployment_id = $1
                  AND rt.oauth_client_id = $2
            )
            UPDATE oauth_refresh_tokens rt
            SET revoked_at = NOW()
            FROM token_chain
            WHERE rt.id = token_chain.id
              AND rt.revoked_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.root_refresh_token_id)
        .execute(&mut *conn)
        .await?;

        Ok(res.rows_affected())
    }
}

pub struct RevokeOAuthTokensByGrant {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub oauth_grant_id: i64,
}

impl RevokeOAuthTokensByGrant {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query(
            r#"
            UPDATE oauth_access_tokens
            SET revoked_at = NOW()
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND oauth_grant_id = $3
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.oauth_grant_id)
        .execute(&mut *conn)
        .await?;

        sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET revoked_at = NOW()
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND oauth_grant_id = $3
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.oauth_grant_id)
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
