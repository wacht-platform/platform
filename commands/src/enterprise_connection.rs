use chrono::Utc;
use common::error::AppError;
use models::enterprise_connection::{EnterpriseConnection, EnterpriseConnectionProtocol};
use serde::{Deserialize, Serialize};

/// Trim `Some("   ")` down to `None` so partial-update COALESCE logic can
/// reliably distinguish "leave field untouched" from "client sent whitespace".
fn normalize_trimmed(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

/// Parse + canonicalise an OIDC issuer URL. Spec requires http/https only.
/// Returns the trimmed form with no trailing slash so all rows store the same
/// canonical value (matters when frontend-api compares stored issuer against
/// the `iss` claim on an id_token).
fn normalize_oidc_issuer_url(raw: &str) -> Result<String, AppError> {
    let parsed = url::Url::parse(raw.trim())
        .map_err(|e| AppError::Validation(format!("oidc_issuer_url is not a valid URL: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(AppError::Validation(format!(
                "oidc_issuer_url scheme `{other}` is not supported; must be http or https"
            )));
        }
    }
    let canonical = parsed.as_str().trim_end_matches('/').to_string();
    Ok(canonical)
}

/// Shared validator for the OIDC fields on enterprise SSO connections.
/// Mutates the option fields in place so the caller binds the normalized
/// values into SQL. Called from both create and update paths.
///
/// `require_all` is true on create (with Oidc protocol) — both issuer_url +
/// client_id MUST be present. On update we call it with `require_all=false`
/// because the existing row may already supply them; the merge check happens
/// in the update command separately.
fn validate_and_normalize_oidc_fields(
    protocol: &EnterpriseConnectionProtocol,
    oidc_issuer_url: &mut Option<String>,
    oidc_client_id: &mut Option<String>,
    oidc_client_secret: &mut Option<String>,
    require_all: bool,
) -> Result<(), AppError> {
    *oidc_issuer_url = normalize_trimmed(oidc_issuer_url.take());
    *oidc_client_id = normalize_trimmed(oidc_client_id.take());
    *oidc_client_secret = normalize_trimmed(oidc_client_secret.take());

    if let Some(url) = oidc_issuer_url.as_ref() {
        *oidc_issuer_url = Some(normalize_oidc_issuer_url(url)?);
    }

    if *protocol == EnterpriseConnectionProtocol::Oidc && require_all {
        if oidc_issuer_url.is_none() {
            return Err(AppError::Validation(
                "oidc_issuer_url is required for OIDC enterprise connections".to_string(),
            ));
        }
        if oidc_client_id.is_none() {
            return Err(AppError::Validation(
                "oidc_client_id is required for OIDC enterprise connections".to_string(),
            ));
        }
        // oidc_client_secret stays optional — public OIDC clients are valid.
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateEnterpriseConnectionRequest {
    #[serde(default)]
    pub organization_id: i64,
    pub domain_id: i64,
    pub protocol: EnterpriseConnectionProtocol,
    pub idp_entity_id: Option<String>,
    pub idp_sso_url: Option<String>,
    pub idp_certificate: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_scopes: Option<String>,
    pub jit_enabled: Option<bool>,
    pub attribute_mapping: Option<serde_json::Value>,
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
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let connection_id = self
            .connection_id
            .ok_or_else(|| AppError::Validation("connection_id is required".to_string()))?;

        let mut request = self.request;
        if request.domain_id == 0 {
            return Err(AppError::Validation(
                "domain_id is required; an enterprise connection must be bound to a verified organization domain"
                    .to_string(),
            ));
        }

        validate_and_normalize_oidc_fields(
            &request.protocol,
            &mut request.oidc_issuer_url,
            &mut request.oidc_client_id,
            &mut request.oidc_client_secret,
            /* require_all = */ true,
        )?;

        let domain_ok = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM organization_domains
                WHERE id = $1 AND organization_id = $2 AND deployment_id = $3 AND verified = true
            ) as "exists!"
            "#,
            request.domain_id,
            request.organization_id,
            self.deployment_id,
        )
        .fetch_one(executor)
        .await?;

        if !domain_ok {
            return Err(AppError::Validation(
                "Domain must be verified for this organization before configuring SSO".to_string(),
            ));
        }

        let jit_enabled = request.jit_enabled.unwrap_or(true);
        let attribute_mapping = request
            .attribute_mapping
            .unwrap_or_else(|| serde_json::json!({}));

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
                oidc_client_id,
                oidc_client_secret,
                oidc_issuer_url,
                oidc_scopes,
                jit_enabled,
                attribute_mapping,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            RETURNING *
            "#,
        )
        .bind(connection_id)
        .bind(request.organization_id)
        .bind(self.deployment_id)
        .bind(request.domain_id)
        .bind(request.protocol)
        .bind(request.idp_entity_id)
        .bind(request.idp_sso_url)
        .bind(request.idp_certificate)
        .bind(request.oidc_client_id)
        .bind(request.oidc_client_secret)
        .bind(request.oidc_issuer_url)
        .bind(request.oidc_scopes)
        .bind(jit_enabled)
        .bind(attribute_mapping)
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
    pub domain_id: Option<i64>,
    pub idp_entity_id: Option<String>,
    pub idp_sso_url: Option<String>,
    pub idp_certificate: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_scopes: Option<String>,
    pub jit_enabled: Option<bool>,
    pub attribute_mapping: Option<serde_json::Value>,
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
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let mut request = self.request;
        // Read the existing protocol so we know whether OIDC field semantics
        // apply. Missing values in the payload are fine on update — the
        // existing row already supplies them — so we only validate URL shape
        // / non-empty for whatever the caller did send.
        let existing_protocol: Option<EnterpriseConnectionProtocol> = sqlx::query_scalar!(
            r#"
            SELECT protocol as "protocol!: String"
            FROM enterprise_connections
            WHERE id = $1 AND organization_id = $2 AND deployment_id = $3
            "#,
            request.connection_id,
            request.organization_id,
            self.deployment_id,
        )
        .fetch_optional(executor)
        .await?
        .and_then(|raw| EnterpriseConnectionProtocol::try_from(raw).ok());
        let protocol_for_validation = existing_protocol
            .unwrap_or(EnterpriseConnectionProtocol::Oidc);
        validate_and_normalize_oidc_fields(
            &protocol_for_validation,
            &mut request.oidc_issuer_url,
            &mut request.oidc_client_id,
            &mut request.oidc_client_secret,
            /* require_all = */ false,
        )?;

        if let Some(domain_id) = request.domain_id {
            if domain_id == 0 {
                return Err(AppError::Validation(
                    "domain_id cannot be cleared; an enterprise connection must remain bound to a verified domain"
                        .to_string(),
                ));
            }
            let domain_ok = sqlx::query_scalar!(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM organization_domains
                    WHERE id = $1 AND organization_id = $2 AND deployment_id = $3 AND verified = true
                ) as "exists!"
                "#,
                domain_id,
                request.organization_id,
                self.deployment_id,
            )
            .fetch_one(executor)
            .await?;
            if !domain_ok {
                return Err(AppError::Validation(
                    "Domain must be verified for this organization before configuring SSO"
                        .to_string(),
                ));
            }
        }

        let connection = sqlx::query_as::<_, EnterpriseConnection>(
            r#"
            UPDATE enterprise_connections
            SET
                domain_id = COALESCE($1, domain_id),
                idp_entity_id = COALESCE($2, idp_entity_id),
                idp_sso_url = COALESCE($3, idp_sso_url),
                idp_certificate = COALESCE($4, idp_certificate),
                oidc_client_id = COALESCE($5, oidc_client_id),
                oidc_client_secret = COALESCE($6, oidc_client_secret),
                oidc_issuer_url = COALESCE($7, oidc_issuer_url),
                oidc_scopes = COALESCE($8, oidc_scopes),
                jit_enabled = COALESCE($9, jit_enabled),
                attribute_mapping = COALESCE($10, attribute_mapping),
                updated_at = $11
            WHERE id = $12 AND organization_id = $13 AND deployment_id = $14
            RETURNING *
            "#,
        )
        .bind(request.domain_id)
        .bind(request.idp_entity_id)
        .bind(request.idp_sso_url)
        .bind(request.idp_certificate)
        .bind(request.oidc_client_id)
        .bind(request.oidc_client_secret)
        .bind(request.oidc_issuer_url)
        .bind(request.oidc_scopes)
        .bind(request.jit_enabled)
        .bind(request.attribute_mapping)
        .bind(Utc::now())
        .bind(request.connection_id)
        .bind(request.organization_id)
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
