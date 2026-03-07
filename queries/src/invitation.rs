use common::error::AppError;
use models::DeploymentInvitation;

pub struct GetDeploymentInvitationQuery {
    invitation_id: i64,
}

impl GetDeploymentInvitationQuery {
    pub fn new(invitation_id: i64) -> Self {
        Self { invitation_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<DeploymentInvitation, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, first_name, last_name, email_address, token, expiry
            FROM deployment_invitations
            WHERE id = $1
            "#,
            self.invitation_id
        )
        .fetch_one(executor)
        .await?;

        let invitation = DeploymentInvitation {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id.unwrap_or(0),
            first_name: row.first_name.unwrap_or_default(),
            last_name: row.last_name.unwrap_or_default(),
            email_address: row.email_address.unwrap_or_default(),
            token: row.token,
            expiry: row.expiry.unwrap_or_else(|| chrono::Utc::now()),
        };

        Ok(invitation)
    }
}
