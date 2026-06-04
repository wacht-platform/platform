use common::error::AppError;

/// Rebuilds a single user's row in the `search_users` index (the denormalized
/// tsvector + trigram table the console/org/workspace search reads). This is the
/// one and only owner of the search-index shape; both backends drive it by
/// publishing a `search.sync_user` task that the worker runs through here.
pub struct SyncSearchUserQuery {
    pub user_id: i64,
}

impl SyncSearchUserQuery {
    pub fn new(user_id: i64) -> Self {
        Self { user_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            INSERT INTO search_users (
                user_id, deployment_id, first_name, last_name, username, primary_email,
                all_emails, all_phone_numbers, organization_ids, workspace_ids,
                organization_roles, workspace_roles, profile_picture_url, search_text,
                created_at, updated_at
            )
            SELECT
                u.id,
                u.deployment_id,
                u.first_name,
                u.last_name,
                u.username,
                COALESCE(pe.email_address, ''),
                COALESCE((SELECT jsonb_agg(email_address) FROM user_email_addresses WHERE user_id = u.id), '[]'::jsonb),
                COALESCE((SELECT jsonb_agg(phone_number) FROM user_phone_numbers WHERE user_id = u.id), '[]'::jsonb),
                COALESCE((SELECT jsonb_agg(organization_id) FROM organization_memberships WHERE user_id = u.id AND deleted_at IS NULL), '[]'::jsonb),
                COALESCE((SELECT jsonb_agg(workspace_id) FROM workspace_memberships WHERE user_id = u.id AND deleted_at IS NULL), '[]'::jsonb),
                COALESCE((SELECT jsonb_agg(DISTINCT omr.organization_role_id)
                          FROM organization_membership_roles omr
                          JOIN organization_memberships om ON om.id = omr.organization_membership_id
                          WHERE om.user_id = u.id AND om.deleted_at IS NULL), '[]'::jsonb),
                COALESCE((SELECT jsonb_agg(DISTINCT wmr.workspace_role_id)
                          FROM workspace_membership_roles wmr
                          JOIN workspace_memberships wm ON wm.id = wmr.workspace_membership_id
                          WHERE wm.user_id = u.id AND wm.deleted_at IS NULL), '[]'::jsonb),
                COALESCE(u.profile_picture_url, ''),
                lower(concat_ws(' ',
                    u.first_name, u.last_name, u.username, pe.email_address,
                    (SELECT string_agg(email_address, ' ') FROM user_email_addresses WHERE user_id = u.id),
                    (SELECT string_agg(concat_ws(' ', concat(country_code, phone_number), phone_number), ' ') FROM user_phone_numbers WHERE user_id = u.id)
                )),
                NOW(), NOW()
            FROM users u
            LEFT JOIN user_email_addresses pe ON u.primary_email_address_id = pe.id
            WHERE u.id = $1
            ON CONFLICT (user_id) DO UPDATE SET
                deployment_id       = EXCLUDED.deployment_id,
                first_name          = EXCLUDED.first_name,
                last_name           = EXCLUDED.last_name,
                username            = EXCLUDED.username,
                primary_email       = EXCLUDED.primary_email,
                all_emails          = EXCLUDED.all_emails,
                all_phone_numbers   = EXCLUDED.all_phone_numbers,
                organization_ids    = EXCLUDED.organization_ids,
                workspace_ids       = EXCLUDED.workspace_ids,
                organization_roles  = EXCLUDED.organization_roles,
                workspace_roles     = EXCLUDED.workspace_roles,
                profile_picture_url = EXCLUDED.profile_picture_url,
                search_text         = EXCLUDED.search_text,
                updated_at          = NOW()
            "#,
            self.user_id
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}
