use chrono::Utc;
use common::error::AppError;
use models::enterprise_connection::{EnterpriseConnection, EnterpriseConnectionProtocol};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateEnterpriseConnectionRequest {
    #[serde(default)]
    pub organization_id: i64,
    pub domain_id: Option<i64>,
    pub protocol: EnterpriseConnectionProtocol,
    pub idp_entity_id: Option<String>,
    pub idp_sso_url: Option<String>,
    pub idp_certificate: Option<String>,
}

pub struct CreateEnterpriseConnectionCommand {
    connection_id: Option<i64>,
    deployment_id: i64,
    request: CreateEnterpriseConnectionRequest,
}

#[derive(Default)]
pub struct CreateEnterpriseConnectionCommandBuilder {
    connection_id: Option<i64>,
    deployment_id: Option<i64>,
    request: Option<CreateEnterpriseConnectionRequest>,
}

impl CreateEnterpriseConnectionCommand {
    pub fn builder() -> CreateEnterpriseConnectionCommandBuilder {
        CreateEnterpriseConnectionCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, request: CreateEnterpriseConnectionRequest) -> Self {
        Self {
            connection_id: None,
            deployment_id,
            request,
        }
    }

    pub fn with_connection_id(mut self, connection_id: i64) -> Self {
        self.connection_id = Some(connection_id);
        self
    }
}

impl CreateEnterpriseConnectionCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<EnterpriseConnection, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let connection_id = self
            .connection_id
            .ok_or_else(|| AppError::Validation("connection_id is required".to_string()))?;
        let now = Utc::now();
        let connection = sqlx::query_as::<_, EnterpriseConnection>(
            r#"
            INSERT INTO enterprise_connections (
                id,
                organization_id,
                deployment_id,
                domain_id,
                protocol,
                idp_entity_id,
                idp_sso_url,
                idp_certificate,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING *
            "#,
        )
        .bind(connection_id)
        .bind(self.request.organization_id)
        .bind(self.deployment_id)
        .bind(self.request.domain_id)
        .bind(self.request.protocol)
        .bind(self.request.idp_entity_id)
        .bind(self.request.idp_sso_url)
        .bind(self.request.idp_certificate)
        .bind(now)
        .bind(now)
        .fetch_one(executor)
        .await?;

        Ok(connection)
    }
}

impl CreateEnterpriseConnectionCommandBuilder {
    pub fn connection_id(mut self, connection_id: i64) -> Self {
        self.connection_id = Some(connection_id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn request(mut self, request: CreateEnterpriseConnectionRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> Result<CreateEnterpriseConnectionCommand, AppError> {
        Ok(CreateEnterpriseConnectionCommand {
            connection_id: self.connection_id,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            request: self
                .request
                .ok_or_else(|| AppError::Validation("request is required".to_string()))?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateEnterpriseConnectionRequest {
    #[serde(default)]
    pub organization_id: i64,
    #[serde(default)]
    pub connection_id: i64,
    pub idp_entity_id: Option<String>,
    pub idp_sso_url: Option<String>,
    pub idp_certificate: Option<String>,
}

pub struct UpdateEnterpriseConnectionCommand {
    deployment_id: i64,
    request: UpdateEnterpriseConnectionRequest,
}

#[derive(Default)]
pub struct UpdateEnterpriseConnectionCommandBuilder {
    deployment_id: Option<i64>,
    request: Option<UpdateEnterpriseConnectionRequest>,
}

impl UpdateEnterpriseConnectionCommand {
    pub fn builder() -> UpdateEnterpriseConnectionCommandBuilder {
        UpdateEnterpriseConnectionCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, request: UpdateEnterpriseConnectionRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl UpdateEnterpriseConnectionCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<EnterpriseConnection, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let connection = sqlx::query_as::<_, EnterpriseConnection>(
            r#"
            UPDATE enterprise_connections
            SET
                idp_entity_id = COALESCE($1, idp_entity_id),
                idp_sso_url = COALESCE($2, idp_sso_url),
                idp_certificate = COALESCE($3, idp_certificate),
                updated_at = $4
            WHERE id = $5 AND organization_id = $6 AND deployment_id = $7
            RETURNING *
            "#,
        )
        .bind(self.request.idp_entity_id)
        .bind(self.request.idp_sso_url)
        .bind(self.request.idp_certificate)
        .bind(Utc::now())
        .bind(self.request.connection_id)
        .bind(self.request.organization_id)
        .bind(self.deployment_id)
        .fetch_one(executor)
        .await?;

        Ok(connection)
    }
}

impl UpdateEnterpriseConnectionCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn request(mut self, request: UpdateEnterpriseConnectionRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> Result<UpdateEnterpriseConnectionCommand, AppError> {
        Ok(UpdateEnterpriseConnectionCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            request: self
                .request
                .ok_or_else(|| AppError::Validation("request is required".to_string()))?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteEnterpriseConnectionRequest {
    pub organization_id: i64,
    pub connection_id: i64,
}

pub struct DeleteEnterpriseConnectionCommand {
    deployment_id: i64,
    request: DeleteEnterpriseConnectionRequest,
}

#[derive(Default)]
pub struct DeleteEnterpriseConnectionCommandBuilder {
    deployment_id: Option<i64>,
    request: Option<DeleteEnterpriseConnectionRequest>,
}

impl DeleteEnterpriseConnectionCommand {
    pub fn builder() -> DeleteEnterpriseConnectionCommandBuilder {
        DeleteEnterpriseConnectionCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, request: DeleteEnterpriseConnectionRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl DeleteEnterpriseConnectionCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM enterprise_connections
            WHERE id = $1 AND organization_id = $2 AND deployment_id = $3
            "#,
            self.request.connection_id,
            self.request.organization_id,
            self.deployment_id
        )
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "Enterprise connection not found".to_string(),
            ));
        }

        Ok(())
    }
}

impl DeleteEnterpriseConnectionCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn request(mut self, request: DeleteEnterpriseConnectionRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> Result<DeleteEnterpriseConnectionCommand, AppError> {
        Ok(DeleteEnterpriseConnectionCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            request: self
                .request
                .ok_or_else(|| AppError::Validation("request is required".to_string()))?,
        })
    }
}
