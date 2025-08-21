use common::error::AppError;
use common::state::AppState;
use models::DeploymentInvitation;

use super::Query;

pub struct GetDeploymentInvitationQuery {
    invitation_id: i64,
}

impl GetDeploymentInvitationQuery {
    pub fn new(invitation_id: i64) -> Self {
        Self { invitation_id }
    }
}

impl Query for GetDeploymentInvitationQuery {
    type Output = DeploymentInvitation;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, first_name, last_name, email_address, expiry
            FROM deployment_invitations
            WHERE id = $1
            "#,
            self.invitation_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let invitation = DeploymentInvitation {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id.unwrap_or(0),
            first_name: row.first_name.unwrap_or_default(),
            last_name: row.last_name.unwrap_or_default(),
            email_address: row.email_address.unwrap_or_default(),
            expiry: row.expiry.unwrap_or_else(|| chrono::Utc::now()),
        };

        Ok(invitation)
    }
}
