use crate::membership_role::insert_organization_membership_role;
use common::error::AppError;
use models::OrganizationMemberDetails;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct AddOrganizationMemberCommand {
    pub membership_id: Option<i64>,
    pub deployment_id: i64,
    pub organization_id: i64,
    pub user_id: i64,
    pub role_ids: Vec<i64>,
}

impl AddOrganizationMemberCommand {
    pub fn new(deployment_id: i64, organization_id: i64, user_id: i64, role_ids: Vec<i64>) -> Self {
        Self {
            membership_id: None,
            deployment_id,
            organization_id,
            user_id,
            role_ids,
        }
    }

    pub fn with_membership_id(mut self, membership_id: i64) -> Self {
        self.membership_id = Some(membership_id);
        self
    }

    pub async fn execute_with_db<'a, A>(
        self,
        acquirer: A,
    ) -> Result<OrganizationMemberDetails, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        let membership_id = self
            .membership_id
            .ok_or_else(|| AppError::Validation("membership_id is required".to_string()))?;
        // Check if user exists
        let user_exists = sqlx::query!("SELECT id FROM users WHERE id = $1", self.user_id)
            .fetch_optional(&mut *tx)
            .await?;

        if user_exists.is_none() {
            return Err(AppError::NotFound("User not found".to_string()));
        }

        // Check if organization exists
        let org_exists = sqlx::query!(
            "SELECT id FROM organizations WHERE deployment_id = $1 AND id = $2",
            self.deployment_id,
            self.organization_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if org_exists.is_none() {
            return Err(AppError::NotFound("Organization not found".to_string()));
        }

        // Check if user is already a member
        let existing_membership = sqlx::query!(
            "SELECT id FROM organization_memberships WHERE organization_id = $1 AND user_id = $2",
            self.organization_id,
            self.user_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if existing_membership.is_some() {
            return Err(AppError::BadRequest(
                "User is already a member of this organization".to_string(),
            ));
        }

        // Create membership
        let membership = sqlx::query!(
            r#"
            INSERT INTO organization_memberships (id, organization_id, user_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, created_at, updated_at
            "#,
            membership_id,
            self.organization_id,
            self.user_id,
            chrono::Utc::now(),
            chrono::Utc::now()
        )
        .fetch_one(&mut *tx)
        .await?;

        // Add role associations
        for role_id in &self.role_ids {
            insert_organization_membership_role(
                &mut *tx,
                membership.id,
                *role_id,
                self.organization_id,
            )
            .await?;
        }

        // Fetch and return the complete member details
        let member_details = sqlx::query!(
            r#"
            SELECT
                om.id, om.created_at, om.updated_at,
                om.organization_id, om.user_id,
                om.public_metadata,
                u.first_name, u.last_name, u.username,
                u.created_at as user_created_at,
                e.email_address as "primary_email_address?",
                p.phone_number as "primary_phone_number?"
            FROM organization_memberships om
            JOIN users u ON om.user_id = u.id
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            WHERE om.id = $1
            "#,
            membership.id
        )
        .fetch_one(&mut *tx)
        .await?;

        let role_rows = sqlx::query!(
            r#"
            SELECT org_role.id, org_role.created_at, org_role.updated_at, org_role.name, org_role.permissions
            FROM organization_membership_roles omr
            JOIN organization_roles org_role ON omr.organization_role_id = org_role.id
            JOIN organization_memberships om ON omr.organization_membership_id = om.id
            WHERE om.organization_id = $1 AND om.user_id = $2
            "#,
            member_details.organization_id,
            member_details.user_id
        )
        .fetch_all(&mut *tx)
        .await?;

        let roles = role_rows
            .into_iter()
            .map(|role_row| models::OrganizationRole {
                id: role_row.id,
                created_at: role_row.created_at,
                updated_at: role_row.updated_at,
                name: role_row.name,
                permissions: role_row.permissions,
                is_deployment_level: false,
            })
            .collect();

        tx.commit().await?;

        Ok(OrganizationMemberDetails {
            id: member_details.id,
            created_at: member_details.created_at,
            updated_at: member_details.updated_at,
            organization_id: member_details.organization_id,
            user_id: member_details.user_id,
            public_metadata: member_details.public_metadata.clone(),
            roles,
            first_name: member_details.first_name,
            last_name: member_details.last_name,
            username: if member_details.username.is_empty() {
                None
            } else {
                Some(member_details.username)
            },
            primary_email_address: member_details.primary_email_address,
            primary_phone_number: member_details.primary_phone_number,
            user_created_at: member_details.user_created_at,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct UpdateOrganizationMemberCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub membership_id: i64,
    pub role_ids: Option<Vec<i64>>,
    pub public_metadata: Option<serde_json::Value>,
}

impl UpdateOrganizationMemberCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let role_ids_present = self.role_ids.is_some();
        let role_ids = self.role_ids.unwrap_or_default();
        let metadata_present = self.public_metadata.is_some();

        let result = sqlx::query!(
            r#"
            WITH membership AS (
                SELECT id
                FROM organization_memberships
                WHERE id = $1
                  AND organization_id = $2
            ),
            cleared_roles AS (
                DELETE FROM organization_membership_roles
                WHERE organization_membership_id = $1
                  AND $3 = true
                  AND EXISTS(SELECT 1 FROM membership)
            ),
            inserted_roles AS (
                INSERT INTO organization_membership_roles (
                    organization_membership_id, organization_role_id, organization_id
                )
                SELECT
                    $1,
                    role_id,
                    $2
                FROM UNNEST($4::BIGINT[]) AS role_id
                WHERE $3 = true
                  AND EXISTS(SELECT 1 FROM membership)
            ),
            updated_metadata AS (
                UPDATE organization_memberships
                SET public_metadata = $5,
                    updated_at = NOW()
                WHERE id = $1
                  AND $6 = true
                  AND EXISTS(SELECT 1 FROM membership)
            )
            SELECT EXISTS(SELECT 1 FROM membership) AS "membership_exists!"
            "#,
            self.membership_id,
            self.organization_id,
            role_ids_present,
            &role_ids,
            self.public_metadata,
            metadata_present
        )
        .fetch_one(executor)
        .await?;

        if !result.membership_exists {
            return Err(AppError::NotFound(
                "Organization membership not found".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct RemoveOrganizationMemberCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub membership_id: i64,
}

impl RemoveOrganizationMemberCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH membership AS (
                SELECT id
                FROM organization_memberships
                WHERE id = $1
                  AND organization_id = $2
            ),
            updated_signins AS (
                UPDATE signins
                SET active_organization_membership_id = NULL,
                    active_workspace_membership_id = NULL
                WHERE active_organization_membership_id = $1
                  AND EXISTS(SELECT 1 FROM membership)
            ),
            deleted_workspace_membership_roles AS (
                DELETE FROM workspace_membership_roles
                WHERE workspace_membership_id IN (
                    SELECT id
                    FROM workspace_memberships
                    WHERE organization_membership_id = $1
                )
                  AND EXISTS(SELECT 1 FROM membership)
            ),
            deleted_workspace_memberships AS (
                DELETE FROM workspace_memberships
                WHERE organization_membership_id = $1
                  AND EXISTS(SELECT 1 FROM membership)
            ),
            deleted_org_membership_roles AS (
                DELETE FROM organization_membership_roles
                WHERE organization_membership_id = $1
                  AND EXISTS(SELECT 1 FROM membership)
            ),
            deleted_membership AS (
                DELETE FROM organization_memberships
                WHERE id = $1
                  AND EXISTS(SELECT 1 FROM membership)
            )
            SELECT EXISTS(SELECT 1 FROM membership) AS "membership_exists!"
            "#,
            self.membership_id,
            self.organization_id
        )
        .fetch_one(executor)
        .await?;

        if !result.membership_exists {
            return Err(AppError::NotFound(
                "Organization membership not found".to_string(),
            ));
        }

        Ok(())
    }
}
