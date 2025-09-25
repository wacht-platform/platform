use super::Query;
use common::error::AppError;
use common::state::AppState;
use models::{
    DeploymentInvitation, DeploymentWaitlistUser, SocialConnection, UserDetails, UserEmailAddress,
    UserPhoneNumber, UserWithIdentifiers,
};
use sqlx::Row;
use std::str::FromStr;

pub struct DeploymentActiveUserListQuery {
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
    deployment_id: i64,
    search: Option<String>,
}

impl DeploymentActiveUserListQuery {
    pub fn new(id: i64) -> Self {
        Self {
            offset: 0,
            sort_key: None,
            sort_order: None,
            limit: 10,
            deployment_id: id,
            search: None,
        }
    }

    pub fn offset(self, offset: i64) -> Self {
        Self { offset, ..self }
    }

    pub fn limit(self, limit: i32) -> Self {
        Self { limit, ..self }
    }

    pub fn sort_key(self, sort_key: Option<String>) -> Self {
        Self { sort_key, ..self }
    }

    pub fn sort_order(self, sort_order: Option<String>) -> Self {
        Self { sort_order, ..self }
    }

    pub fn search(self, search: Option<String>) -> Self {
        Self { search, ..self }
    }
}

pub struct DeploymentInvitationQuery {
    deployment_id: i64,
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
}

impl DeploymentInvitationQuery {
    pub fn new(id: i64) -> Self {
        Self {
            offset: 0,
            sort_key: None,
            sort_order: None,
            limit: 10,
            deployment_id: id,
        }
    }

    pub fn offset(self, offset: i64) -> Self {
        Self { offset, ..self }
    }

    pub fn limit(self, limit: i32) -> Self {
        Self { limit, ..self }
    }

    pub fn sort_key(self, sort_key: Option<String>) -> Self {
        Self { sort_key, ..self }
    }

    pub fn sort_order(self, sort_order: Option<String>) -> Self {
        Self { sort_order, ..self }
    }
}

pub struct DeploymentWaitlistQuery {
    deployment_id: i64,
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
}

impl DeploymentWaitlistQuery {
    pub fn new(id: i64) -> Self {
        Self {
            offset: 0,
            sort_key: None,
            sort_order: None,
            limit: 10,
            deployment_id: id,
        }
    }

    pub fn offset(self, offset: i64) -> Self {
        Self { offset, ..self }
    }

    pub fn limit(self, limit: i32) -> Self {
        Self { limit, ..self }
    }

    pub fn sort_key(self, sort_key: Option<String>) -> Self {
        Self { sort_key, ..self }
    }

    pub fn sort_order(self, sort_order: Option<String>) -> Self {
        Self { sort_order, ..self }
    }
}

impl Query for DeploymentActiveUserListQuery {
    type Output = Vec<UserWithIdentifiers>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");

        let mut query_builder = sqlx::QueryBuilder::new(
            r#"
            SELECT
                u.id, u.created_at, u.updated_at,
                u.first_name, u.last_name, u.username, u.profile_picture_url,
                e.email_address as primary_email_address,
                p.phone_number as primary_phone_number
            FROM users u
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            WHERE u.deployment_id = "#,
        );

        query_builder.push_bind(self.deployment_id);
        query_builder.push(" AND u.deleted_at IS NULL");

        if let Some(search_term) = &self.search {
            let trimmed_search = search_term.trim();
            if !trimmed_search.is_empty() {
                let search_pattern = format!("%{}%", trimmed_search);

                query_builder.push(" AND (");

                query_builder.push("u.first_name ILIKE ");
                query_builder.push_bind(search_pattern.clone());

                query_builder.push(" OR ");

                query_builder.push("u.last_name ILIKE ");
                query_builder.push_bind(search_pattern.clone());

                query_builder.push(" OR ");

                query_builder.push("u.username ILIKE ");
                query_builder.push_bind(search_pattern);

                query_builder.push(")");
            }
        }

        query_builder.push(" ORDER BY ");

        match sort_key {
            "created_at" => query_builder.push("u.created_at"),
            "username" => query_builder.push("u.username"),
            "email" => query_builder.push("e.email_address"),
            "phone_number" => query_builder.push("p.phone_number"),
            _ => query_builder.push("u.created_at"),
        };

        match sort_order.to_lowercase().as_str() {
            "asc" => query_builder.push(" ASC"),
            _ => query_builder.push(" DESC"),
        };

        query_builder.push(" OFFSET ");
        query_builder.push_bind(self.offset);
        query_builder.push(" LIMIT ");
        query_builder.push_bind(self.limit);

        let rows = query_builder.build().fetch_all(&app_state.db_pool).await?;

        let users = rows
            .into_iter()
            .map(|row| UserWithIdentifiers {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                first_name: row.get("first_name"),
                last_name: row.get("last_name"),
                username: row.get("username"),
                profile_picture_url: row.get("profile_picture_url"),
                primary_email_address: row.get("primary_email_address"),
                primary_phone_number: row.get("primary_phone_number"),
            })
            .collect();

        Ok(users)
    }
}

impl Query for DeploymentInvitationQuery {
    type Output = Vec<DeploymentInvitation>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");

        let mut query_builder = sqlx::QueryBuilder::new(
            r#"
            SELECT
                i.id, i.created_at, i.updated_at,
                i.first_name, i.last_name,
                i.email_address, i.deployment_id,
                i.token, i.expiry
            FROM deployment_invitations i
            WHERE i.deployment_id = "#,
        );

        query_builder.push_bind(self.deployment_id);

        query_builder.push(" ORDER BY ");

        match sort_key {
            "created_at" => query_builder.push("i.created_at"),
            "email" => query_builder.push("i.email_address"),
            _ => query_builder.push("i.created_at"),
        };

        match sort_order.to_lowercase().as_str() {
            "asc" => query_builder.push(" ASC"),
            _ => query_builder.push(" DESC"),
        };

        query_builder.push(" OFFSET ");
        query_builder.push_bind(self.offset);
        query_builder.push(" LIMIT ");
        query_builder.push_bind(self.limit);

        let rows = query_builder.build().fetch_all(&app_state.db_pool).await?;

        let invitations = rows
            .into_iter()
            .map(|row| DeploymentInvitation {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                first_name: row.get("first_name"),
                last_name: row.get("last_name"),
                deployment_id: row.get("deployment_id"),
                email_address: row.get("email_address"),
                token: row.get("token"),
                expiry: row.get("expiry"),
            })
            .collect();

        Ok(invitations)
    }
}

pub struct GetUserDetailsQuery {
    deployment_id: i64,
    user_id: i64,
}

impl GetUserDetailsQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }
}

impl Query for GetUserDetailsQuery {
    type Output = UserDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let user_row = sqlx::query!(
            r#"
            SELECT
                u.id, u.created_at, u.updated_at,
                u.first_name, u.last_name, u.username, u.profile_picture_url,
                u.schema_version, u.disabled, u.second_factor_policy,
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
        .fetch_one(&app_state.db_pool)
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
        .fetch_all(&app_state.db_pool)
        .await?;

        let email_addresses = email_rows
            .into_iter()
            .map(|row| UserEmailAddress {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                deployment_id: row.deployment_id.unwrap_or(self.deployment_id),
                user_id: row.user_id.unwrap_or(self.user_id),
                email: row.email.unwrap_or_default(),
                is_primary: row.is_primary,
                verified: row.verified,
                verified_at: row.verified_at.unwrap_or_else(|| chrono::Utc::now()),
                verification_strategy: row
                    .verification_strategy
                    .and_then(|s| models::VerificationStrategy::from_str(&s).ok())
                    .unwrap_or(models::VerificationStrategy::Otp),
            })
            .collect();

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
        .fetch_all(&app_state.db_pool)
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
                verified_at: row.verified_at.unwrap_or_else(|| chrono::Utc::now()),
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
        .fetch_all(&app_state.db_pool)
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
            has_password: user_row.password.is_some()
                && !user_row.password.unwrap_or_default().is_empty(),
            has_backup_codes: user_row.backup_codes.is_some()
                && !user_row.backup_codes.unwrap_or_default().is_empty(),
        };

        Ok(user_details)
    }
}

impl Query for DeploymentWaitlistQuery {
    type Output = Vec<DeploymentWaitlistUser>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");

        let mut query_builder = sqlx::QueryBuilder::new(
            r#"
            SELECT
                u.id, u.created_at, u.updated_at,
                u.email_address, u.first_name, u.last_name,
                u.deployment_id
            FROM deployment_waitlist_users u
            WHERE u.deployment_id = "#,
        );

        query_builder.push_bind(self.deployment_id);

        query_builder.push(" ORDER BY ");

        match sort_key {
            "created_at" => query_builder.push("u.created_at"),
            "email" => query_builder.push("u.email_address"),
            _ => query_builder.push("u.created_at"),
        };

        match sort_order.to_lowercase().as_str() {
            "asc" => query_builder.push(" ASC"),
            _ => query_builder.push(" DESC"),
        };

        query_builder.push(" OFFSET ");
        query_builder.push_bind(self.offset);
        query_builder.push(" LIMIT ");
        query_builder.push_bind(self.limit);

        let rows = query_builder.build().fetch_all(&app_state.db_pool).await?;

        let waitlist_users = rows
            .into_iter()
            .map(|row| DeploymentWaitlistUser {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                first_name: row.get("first_name"),
                last_name: row.get("last_name"),
                email_address: row.get("email_address"),
                deployment_id: row.get("deployment_id"),
            })
            .collect();

        Ok(waitlist_users)
    }
}
