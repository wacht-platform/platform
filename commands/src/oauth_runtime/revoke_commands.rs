use crate::Command;
use common::error::AppError;
use common::state::AppState;

pub struct RevokeOAuthRefreshTokenById {
    pub refresh_token_id: i64,
}

impl Command for RevokeOAuthRefreshTokenById {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let res = sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET revoked_at = NOW()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.refresh_token_id)
        .execute(&app_state.db_pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}

pub struct SetOAuthRefreshTokenReplacement {
    pub old_refresh_token_id: i64,
    pub new_refresh_token_id: i64,
}

impl Command for SetOAuthRefreshTokenReplacement {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET replaced_by_token_id = $2
            WHERE id = $1
            "#,
        )
        .bind(self.old_refresh_token_id)
        .bind(self.new_refresh_token_id)
        .execute(&app_state.db_pool)
        .await?;
        Ok(())
    }
}

pub struct RevokeOAuthAccessTokenByHash {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub token_hash: String,
}

impl Command for RevokeOAuthAccessTokenByHash {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .execute(&app_state.db_pool)
        .await?;
        Ok(())
    }
}

pub struct RevokeOAuthRefreshTokenByHash {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub token_hash: String,
}

impl Command for RevokeOAuthRefreshTokenByHash {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .execute(&app_state.db_pool)
        .await?;
        Ok(())
    }
}

pub struct RevokeOAuthRefreshTokenFamily {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub root_refresh_token_id: i64,
}

impl Command for RevokeOAuthRefreshTokenFamily {
    type Output = u64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .execute(&app_state.db_pool)
        .await?;

        Ok(res.rows_affected())
    }
}

pub struct RevokeOAuthTokensByGrant {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub oauth_grant_id: i64,
}

impl Command for RevokeOAuthTokensByGrant {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .execute(&app_state.db_pool)
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
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}
