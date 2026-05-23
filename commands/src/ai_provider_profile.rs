use common::{HasDbRouter, HasEncryptionProvider, error::AppError};
use models::{
    CreateDeploymentAiProviderProfileRequest, DeploymentAiProviderProfile, DeploymentLlmProvider,
    UpdateDeploymentAiProviderProfileRequest,
};

fn provider_as_str(provider: &DeploymentLlmProvider) -> Result<&'static str, AppError> {
    match provider {
        DeploymentLlmProvider::Openai => Ok("openai"),
        _ => Err(AppError::BadRequest(
            "AI provider profiles currently support provider=openai".to_string(),
        )),
    }
}

fn normalize_required(value: String, field: &str) -> Result<String, AppError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(AppError::BadRequest(format!("{field} cannot be empty")));
    }
    Ok(value)
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub struct CreateDeploymentAiProviderProfileCommand {
    id: i64,
    deployment_id: i64,
    request: CreateDeploymentAiProviderProfileRequest,
}

impl CreateDeploymentAiProviderProfileCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        request: CreateDeploymentAiProviderProfileRequest,
    ) -> Self {
        Self {
            id,
            deployment_id,
            request,
        }
    }

    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<DeploymentAiProviderProfile, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider,
    {
        let provider = provider_as_str(&self.request.provider)?;
        let name = normalize_required(self.request.name, "name")?;
        let slug = normalize_required(self.request.slug, "slug")?;
        let api_key = normalize_required(self.request.api_key, "api_key")?;
        let encrypted_api_key = deps.encryption_provider().encrypt(&api_key)?;

        sqlx::query_as!(
            DeploymentAiProviderProfile,
            r#"
            INSERT INTO deployment_ai_provider_profiles (
                id, deployment_id, provider, name, slug, api_key, base_url,
                organization, project, default_model, enabled
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, COALESCE($11, TRUE))
            RETURNING
                id, deployment_id, provider, name, slug, api_key, base_url,
                organization, project, default_model, enabled, created_at, updated_at
            "#,
            self.id,
            self.deployment_id,
            provider,
            name,
            slug,
            encrypted_api_key,
            normalize_optional(self.request.base_url),
            normalize_optional(self.request.organization),
            normalize_optional(self.request.project),
            normalize_optional(self.request.default_model),
            self.request.enabled,
        )
        .fetch_one(deps.db_router().writer())
        .await
        .map_err(AppError::Database)
    }
}

pub struct UpdateDeploymentAiProviderProfileCommand {
    deployment_id: i64,
    profile_id: i64,
    request: UpdateDeploymentAiProviderProfileRequest,
}

impl UpdateDeploymentAiProviderProfileCommand {
    pub fn new(
        deployment_id: i64,
        profile_id: i64,
        request: UpdateDeploymentAiProviderProfileRequest,
    ) -> Self {
        Self {
            deployment_id,
            profile_id,
            request,
        }
    }

    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<DeploymentAiProviderProfile, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider,
    {
        let encrypted_api_key = normalize_optional(self.request.api_key)
            .map(|value| deps.encryption_provider().encrypt(&value))
            .transpose()?;

        sqlx::query_as!(
            DeploymentAiProviderProfile,
            r#"
            UPDATE deployment_ai_provider_profiles
            SET
                name = COALESCE($3, name),
                slug = COALESCE($4, slug),
                api_key = COALESCE($5, api_key),
                base_url = COALESCE($6, base_url),
                organization = COALESCE($7, organization),
                project = COALESCE($8, project),
                default_model = COALESCE($9, default_model),
                enabled = COALESCE($10, enabled),
                updated_at = NOW()
            WHERE id = $1 AND deployment_id = $2
            RETURNING
                id, deployment_id, provider, name, slug, api_key, base_url,
                organization, project, default_model, enabled, created_at, updated_at
            "#,
            self.profile_id,
            self.deployment_id,
            normalize_optional(self.request.name),
            normalize_optional(self.request.slug),
            encrypted_api_key,
            normalize_optional(self.request.base_url),
            normalize_optional(self.request.organization),
            normalize_optional(self.request.project),
            normalize_optional(self.request.default_model),
            self.request.enabled,
        )
        .fetch_optional(deps.db_router().writer())
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("AI provider profile not found".to_string()))
    }
}

pub struct DeleteDeploymentAiProviderProfileCommand {
    deployment_id: i64,
    profile_id: i64,
}

impl DeleteDeploymentAiProviderProfileCommand {
    pub fn new(deployment_id: i64, profile_id: i64) -> Self {
        Self {
            deployment_id,
            profile_id,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM deployment_ai_provider_profiles
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.profile_id,
            self.deployment_id
        )
        .execute(deps.db_router().writer())
        .await
        .map_err(AppError::Database)?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "AI provider profile not found".to_string(),
            ));
        }
        Ok(())
    }
}
