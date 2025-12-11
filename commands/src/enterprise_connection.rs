use crate::Command;
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
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
    pub deployment_id: i64,
    pub request: CreateEnterpriseConnectionRequest,
}

impl CreateEnterpriseConnectionCommand {
    pub fn new(deployment_id: i64, request: CreateEnterpriseConnectionRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl Command for CreateEnterpriseConnectionCommand {
    type Output = EnterpriseConnection;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let connection_id = app_state.sf.next_id()? as i64;
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
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(connection)
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
    pub deployment_id: i64,
    pub request: UpdateEnterpriseConnectionRequest,
}

impl UpdateEnterpriseConnectionCommand {
    pub fn new(deployment_id: i64, request: UpdateEnterpriseConnectionRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl Command for UpdateEnterpriseConnectionCommand {
    type Output = EnterpriseConnection;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(connection)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteEnterpriseConnectionRequest {
    pub organization_id: i64,
    pub connection_id: i64,
}

pub struct DeleteEnterpriseConnectionCommand {
    pub deployment_id: i64,
    pub request: DeleteEnterpriseConnectionRequest,
}

impl DeleteEnterpriseConnectionCommand {
    pub fn new(deployment_id: i64, request: DeleteEnterpriseConnectionRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl Command for DeleteEnterpriseConnectionCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            DELETE FROM enterprise_connections
            WHERE id = $1 AND organization_id = $2 AND deployment_id = $3
            "#,
            self.request.connection_id,
            self.request.organization_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "Enterprise connection not found".to_string(),
            ));
        }

        Ok(())
    }
}
