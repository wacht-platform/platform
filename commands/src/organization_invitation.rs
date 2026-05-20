use common::error::AppError;
use rand::RngCore;

/// Hex-encodes 32 random bytes and prefixes with `org.` — matches the format
/// frontend-api emits (utils.GenerateSecureToken + "org." prefix) so accept
/// links remain interchangeable across both code paths.
fn generate_invitation_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("org.{}", hex::encode(bytes))
}

pub struct CreatedOrganizationInvitation {
    pub id: i64,
    pub token: String,
    pub email: String,
    pub workspace_id: Option<i64>,
    pub organization_name: String,
}

pub struct CreateOrganizationInvitationCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub invitation_id: i64,
    pub email: String,
    pub initial_organization_role_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub initial_workspace_role_id: Option<i64>,
    pub expiry_days: i64,
}

impl CreateOrganizationInvitationCommand {
    pub async fn execute_with_pool(
        self,
        pool: &sqlx::PgPool,
    ) -> Result<CreatedOrganizationInvitation, AppError> {
        let email = self.email.trim().to_ascii_lowercase();
        if email.is_empty() {
            return Err(AppError::BadRequest("email is required".to_string()));
        }

        let mut tx = pool.begin().await?;

        // Validate org belongs to deployment + fetch its name for the email.
        let org = sqlx::query!(
            r#"
            SELECT id, name
            FROM organizations
            WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL
            FOR UPDATE
            "#,
            self.organization_id,
            self.deployment_id,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let org = org.ok_or_else(|| AppError::NotFound("organization not found".to_string()))?;

        // If a workspace_id was supplied, validate it belongs to this org.
        if let Some(ws_id) = self.workspace_id {
            let ws_ok = sqlx::query_scalar!(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM workspaces
                    WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL
                ) AS "exists!"
                "#,
                ws_id,
                self.organization_id,
            )
            .fetch_one(&mut *tx)
            .await?;
            if !ws_ok {
                return Err(AppError::BadRequest(
                    "workspace does not belong to this organization".to_string(),
                ));
            }
        }

        // Reject if there's already a pending invitation for this email scope
        // OR the email already belongs to a member.
        let conflict = sqlx::query!(
            r#"
            SELECT
                EXISTS(
                    SELECT 1 FROM organization_invitations
                    WHERE email = $1
                      AND organization_id = $2
                      AND workspace_id IS NOT DISTINCT FROM $3
                      AND deleted_at IS NULL
                ) AS "has_pending!",
                EXISTS(
                    SELECT 1 FROM organization_memberships om
                    JOIN user_email_addresses uea ON uea.user_id = om.user_id
                    WHERE uea.email_address = $1
                      AND uea.deployment_id = $4
                      AND om.organization_id = $2
                      AND om.deleted_at IS NULL
                ) AS "is_member!"
            "#,
            email,
            self.organization_id,
            self.workspace_id,
            self.deployment_id,
        )
        .fetch_one(&mut *tx)
        .await?;

        if conflict.has_pending {
            return Err(AppError::Conflict(
                "an invitation for this email is already pending".to_string(),
            ));
        }
        if conflict.is_member {
            return Err(AppError::Conflict(
                "this email already belongs to a member of the organization".to_string(),
            ));
        }

        let token = generate_invitation_token();
        let expiry = chrono::Utc::now() + chrono::Duration::days(self.expiry_days);

        sqlx::query!(
            r#"
            INSERT INTO organization_invitations (
                id, created_at, updated_at,
                organization_id, email,
                initial_organization_role_id,
                workspace_id, initial_workspace_role_id,
                expired, expiry, token
            )
            VALUES ($1, NOW(), NOW(), $2, $3, $4, $5, $6, false, $7, $8)
            "#,
            self.invitation_id,
            self.organization_id,
            email,
            self.initial_organization_role_id,
            self.workspace_id,
            self.initial_workspace_role_id,
            expiry,
            token,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(CreatedOrganizationInvitation {
            id: self.invitation_id,
            token,
            email,
            workspace_id: self.workspace_id,
            organization_name: org.name,
        })
    }
}

pub struct DiscardOrganizationInvitationCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub invitation_id: i64,
}

impl DiscardOrganizationInvitationCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            UPDATE organization_invitations
            SET deleted_at = NOW(), updated_at = NOW()
            FROM organizations o
            WHERE organization_invitations.organization_id = o.id
              AND o.deployment_id = $1
              AND organization_invitations.organization_id = $2
              AND organization_invitations.id = $3
              AND organization_invitations.deleted_at IS NULL
            "#,
            self.deployment_id,
            self.organization_id,
            self.invitation_id,
        )
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("invitation not found".to_string()));
        }
        Ok(())
    }
}
