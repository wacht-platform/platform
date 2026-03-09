use super::*;

pub struct ListOAuthGrantsByClientQuery {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
}

impl ListOAuthGrantsByClientQuery {
    pub fn new(deployment_id: i64, oauth_client_id: i64) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<OAuthClientGrantData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT
                g.id,
                g.deployment_id,
                g.app_slug as api_auth_app_slug,
                g.oauth_client_id,
                g.resource,
                g.scopes as "scopes: serde_json::Value",
                g.status,
                g.granted_at,
                g.expires_at,
                g.revoked_at,
                g.granted_by_user_id,
                g.created_at,
                g.updated_at
            FROM oauth_client_grants g
            WHERE g.deployment_id = $1
              AND g.oauth_client_id = $2
            ORDER BY g.created_at DESC
            "#,
            self.deployment_id,
            self.oauth_client_id
        )
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OAuthClientGrantData {
                id: r.id,
                deployment_id: r.deployment_id,
                api_auth_app_slug: r.api_auth_app_slug,
                oauth_client_id: r.oauth_client_id,
                resource: r.resource,
                scopes: r.scopes,
                status: r.status,
                granted_at: r.granted_at,
                expires_at: r.expires_at,
                revoked_at: r.revoked_at,
                granted_by_user_id: r.granted_by_user_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}
