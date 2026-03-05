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

#[derive(Default)]
pub struct ListOrganizationDomainsQueryBuilder {
    deployment_id: Option<i64>,
    organization_id: Option<i64>,
    limit: Option<i32>,
    offset: Option<i64>,
}

impl ListOrganizationDomainsQuery {
    pub fn builder() -> ListOrganizationDomainsQueryBuilder {
        ListOrganizationDomainsQueryBuilder::default()
    }

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
        self.execute_with(&app_state.db_pool).await
    }
}

impl ListOrganizationDomainsQuery {
    pub async fn execute_with(
        &self,
        pool: &sqlx::PgPool,
    ) -> Result<Vec<OrganizationDomain>, AppError> {
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
        .fetch_all(pool)
        .await?;

        Ok(domains)
    }
}

impl ListOrganizationDomainsQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn organization_id(mut self, organization_id: i64) -> Self {
        self.organization_id = Some(organization_id);
        self
    }

    pub fn limit(mut self, limit: i32) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn build(self) -> Result<ListOrganizationDomainsQuery, AppError> {
        Ok(ListOrganizationDomainsQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            organization_id: self
                .organization_id
                .ok_or_else(|| AppError::Validation("organization_id is required".to_string()))?,
            limit: self.limit.unwrap_or(50),
            offset: self.offset.unwrap_or(0),
        })
    }
}

pub struct ListEnterpriseConnectionsQuery {
    deployment_id: i64,
    organization_id: i64,
    limit: i32,
    offset: i64,
}

#[derive(Default)]
pub struct ListEnterpriseConnectionsQueryBuilder {
    deployment_id: Option<i64>,
    organization_id: Option<i64>,
    limit: Option<i32>,
    offset: Option<i64>,
}

impl ListEnterpriseConnectionsQuery {
    pub fn builder() -> ListEnterpriseConnectionsQueryBuilder {
        ListEnterpriseConnectionsQueryBuilder::default()
    }

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
        self.execute_with(&app_state.db_pool).await
    }
}

impl ListEnterpriseConnectionsQuery {
    pub async fn execute_with(
        &self,
        pool: &sqlx::PgPool,
    ) -> Result<Vec<EnterpriseConnection>, AppError> {
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
        .fetch_all(pool)
        .await?;

        Ok(connections)
    }
}

impl ListEnterpriseConnectionsQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn organization_id(mut self, organization_id: i64) -> Self {
        self.organization_id = Some(organization_id);
        self
    }

    pub fn limit(mut self, limit: i32) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn build(self) -> Result<ListEnterpriseConnectionsQuery, AppError> {
        Ok(ListEnterpriseConnectionsQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            organization_id: self
                .organization_id
                .ok_or_else(|| AppError::Validation("organization_id is required".to_string()))?,
            limit: self.limit.unwrap_or(50),
            offset: self.offset.unwrap_or(0),
        })
    }
}

pub struct GetScimTokenQuery {
    deployment_id: i64,
    organization_id: i64,
    connection_id: i64,
}

#[derive(Default)]
pub struct GetScimTokenQueryBuilder {
    deployment_id: Option<i64>,
    organization_id: Option<i64>,
    connection_id: Option<i64>,
}

impl GetScimTokenQuery {
    pub fn builder() -> GetScimTokenQueryBuilder {
        GetScimTokenQueryBuilder::default()
    }

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
        self.execute_with(&app_state.db_pool).await
    }
}

impl GetScimTokenQuery {
    pub async fn execute_with(
        &self,
        pool: &sqlx::PgPool,
    ) -> Result<Option<models::scim_token::ScimToken>, AppError> {
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
        .fetch_optional(pool)
        .await?;

        Ok(token)
    }
}

impl GetScimTokenQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn organization_id(mut self, organization_id: i64) -> Self {
        self.organization_id = Some(organization_id);
        self
    }

    pub fn connection_id(mut self, connection_id: i64) -> Self {
        self.connection_id = Some(connection_id);
        self
    }

    pub fn build(self) -> Result<GetScimTokenQuery, AppError> {
        Ok(GetScimTokenQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            organization_id: self
                .organization_id
                .ok_or_else(|| AppError::Validation("organization_id is required".to_string()))?,
            connection_id: self
                .connection_id
                .ok_or_else(|| AppError::Validation("connection_id is required".to_string()))?,
        })
    }
}
