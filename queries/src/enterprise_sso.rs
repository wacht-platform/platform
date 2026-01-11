use crate::prelude::*;
use models::{
    enterprise_connection::EnterpriseConnection, organization_domain::OrganizationDomain,
};

pub struct ListOrganizationDomainsQuery {
    deployment_id: i64,
    organization_id: i64,
    limit: i32,
    offset: i64,
}

impl ListOrganizationDomainsQuery {
    pub fn new(deployment_id: i64, organization_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
            limit: 50,
            offset: 0,
        }
    }

    pub fn limit(mut self, limit: i32) -> Self {
        self.limit = limit;
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = offset;
        self
    }
}

impl Query for ListOrganizationDomainsQuery {
    type Output = Vec<OrganizationDomain>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let domains = sqlx::query_as!(
            OrganizationDomain,
            r#"
            SELECT *
            FROM organization_domains
            WHERE deployment_id = $1 AND organization_id = $2
            ORDER BY created_at DESC
            LIMIT $3 OFFSET $4
            "#,
            self.deployment_id,
            self.organization_id,
            self.limit as i64,
            self.offset
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(domains)
    }
}

pub struct ListEnterpriseConnectionsQuery {
    deployment_id: i64,
    organization_id: i64,
    limit: i32,
    offset: i64,
}

impl ListEnterpriseConnectionsQuery {
    pub fn new(deployment_id: i64, organization_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
            limit: 50,
            offset: 0,
        }
    }

    pub fn limit(mut self, limit: i32) -> Self {
        self.limit = limit;
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = offset;
        self
    }
}

impl Query for ListEnterpriseConnectionsQuery {
    type Output = Vec<EnterpriseConnection>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let connections = sqlx::query_as::<_, EnterpriseConnection>(
            r#"
            SELECT *
            FROM enterprise_connections
            WHERE deployment_id = $1 AND organization_id = $2
            ORDER BY created_at DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.organization_id)
        .bind(self.limit as i64)
        .bind(self.offset)
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(connections)
    }
}

pub struct GetScimTokenQuery {
    deployment_id: i64,
    organization_id: i64,
    connection_id: i64,
}

impl GetScimTokenQuery {
    pub fn new(deployment_id: i64, organization_id: i64, connection_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
            connection_id,
        }
    }
}

impl Query for GetScimTokenQuery {
    type Output = Option<models::scim_token::ScimToken>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let token = sqlx::query_as::<_, models::scim_token::ScimToken>(
            r#"
            SELECT *
            FROM scim_tokens
            WHERE enterprise_connection_id = $1 
              AND organization_id = $2 
              AND deployment_id = $3
              AND enabled = true
            "#,
        )
        .bind(self.connection_id)
        .bind(self.organization_id)
        .bind(self.deployment_id)
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(token)
    }
}
