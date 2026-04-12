use aws_config::{BehaviorVersion, Region};
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Credentials};
use aws_sdk_s3::primitives::ByteStream;
use common::{
    HasDbRouter, HasEncryptionProvider, VectorStoreConfig, build_vector_store_config,
    db_router::ReadConsistency, error::AppError,
};
use models::DeploymentStorageProvider;

const DEFAULT_DEPLOYMENT_S3_REGION: &str = "auto";

#[derive(Debug, Clone)]
pub struct PendingDeploymentStorageConfig {
    pub bucket: String,
    pub endpoint: String,
    pub region: String,
    pub root_prefix: Option<String>,
    pub force_path_style: bool,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl PendingDeploymentStorageConfig {
    pub fn object_key(&self, relative_key: &str) -> String {
        let normalized_key = relative_key.trim_start_matches('/').to_string();
        match self.root_prefix.as_deref() {
            Some(prefix) => format!("{}/{}", prefix, normalized_key),
            None => normalized_key,
        }
    }
}

#[derive(Clone)]
pub struct ResolvedDeploymentStorage {
    client: S3Client,
    pub provider: DeploymentStorageProvider,
    pub bucket: String,
    pub endpoint: Option<String>,
    pub region: String,
    pub root_prefix: Option<String>,
    pub force_path_style: bool,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub vector_store_initialized: bool,
}

impl ResolvedDeploymentStorage {
    pub fn client(&self) -> &S3Client {
        &self.client
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn object_key(&self, relative_key: &str) -> String {
        let normalized_key = relative_key.trim_start_matches('/').to_string();
        match self.root_prefix.as_deref() {
            Some(prefix) => format!("{}/{}", prefix, normalized_key),
            None => normalized_key,
        }
    }

    pub fn vector_store_config(&self) -> VectorStoreConfig {
        build_vector_store_config(
            &self.bucket,
            self.root_prefix.as_deref(),
            self.endpoint.as_deref(),
            &self.region,
            self.access_key_id.as_deref(),
            self.secret_access_key.as_deref(),
            self.force_path_style,
        )
    }
}

pub struct ResolveDeploymentStorageCommand {
    deployment_id: i64,
}

impl ResolveDeploymentStorageCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<ResolvedDeploymentStorage, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider + ?Sized,
    {
        let settings = queries::GetDeploymentAiSettingsQuery::new(self.deployment_id)
            .execute_with_db(deps.reader_pool(ReadConsistency::Strong))
            .await?;

        let Some(settings) = settings else {
            return Err(AppError::Validation(
                "Deployment storage is not configured. Customer S3 storage is required."
                    .to_string(),
            ));
        };

        if settings.storage_provider != "s3" {
            return Err(AppError::Validation(
                "Deployment storage provider must be s3. Customer S3 storage is required."
                    .to_string(),
            ));
        }

        resolve_s3_storage(&settings, deps).await
    }
}

pub struct WriteToDeploymentStorageCommand {
    pub deployment_id: i64,
    pub key: String,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

impl WriteToDeploymentStorageCommand {
    pub fn new(deployment_id: i64, key: String, body: Vec<u8>) -> Self {
        Self {
            deployment_id,
            key,
            body,
            content_type: None,
        }
    }

    pub fn with_content_type(mut self, content_type: String) -> Self {
        self.content_type = Some(content_type);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<String, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider + ?Sized,
    {
        let storage = ResolveDeploymentStorageCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;
        let resolved_key = storage.object_key(&self.key);

        let mut request = storage
            .client()
            .put_object()
            .bucket(storage.bucket())
            .key(&resolved_key)
            .body(ByteStream::from(self.body));

        if let Some(content_type) = self.content_type {
            request = request.content_type(content_type);
        }

        request
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        Ok(resolved_key)
    }
}

pub struct DeleteFromDeploymentStorageCommand {
    pub deployment_id: i64,
    pub key: String,
    resolved: bool,
}

impl DeleteFromDeploymentStorageCommand {
    pub fn new(deployment_id: i64, key: String) -> Self {
        Self {
            deployment_id,
            key,
            resolved: false,
        }
    }

    pub fn with_resolved_key(mut self) -> Self {
        self.resolved = true;
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasEncryptionProvider + ?Sized,
    {
        let storage = ResolveDeploymentStorageCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;
        let key = if self.resolved {
            self.key
        } else {
            storage.object_key(&self.key)
        };

        storage
            .client()
            .delete_object()
            .bucket(storage.bucket())
            .key(&key)
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        Ok(())
    }
}

pub struct DeletePrefixFromDeploymentStorageCommand {
    pub deployment_id: i64,
    pub prefix: String,
}

impl DeletePrefixFromDeploymentStorageCommand {
    pub fn new(deployment_id: i64, prefix: String) -> Self {
        Self {
            deployment_id,
            prefix,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasEncryptionProvider + ?Sized,
    {
        let storage = ResolveDeploymentStorageCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;
        let resolved_prefix = storage.object_key(&self.prefix);

        let list_result = storage
            .client()
            .list_objects_v2()
            .bucket(storage.bucket())
            .prefix(&resolved_prefix)
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        if let Some(objects) = list_result.contents {
            for obj in objects {
                if let Some(key) = obj.key {
                    storage
                        .client()
                        .delete_object()
                        .bucket(storage.bucket())
                        .key(&key)
                        .send()
                        .await
                        .map_err(|e| AppError::S3(e.to_string()))?;
                }
            }
        }

        Ok(())
    }
}

async fn resolve_s3_storage<D>(
    settings: &models::DeploymentAiSettings,
    deps: &D,
) -> Result<ResolvedDeploymentStorage, AppError>
where
    D: HasEncryptionProvider + ?Sized,
{
    let bucket = required_storage_field("storage.bucket", settings.storage_bucket.as_deref())?;
    let endpoint =
        required_storage_field("storage.endpoint", settings.storage_endpoint.as_deref())?;
    let access_key_id = required_encrypted_storage_field(
        "storage.access_key_id",
        settings.storage_access_key_id.as_deref(),
        deps,
    )?;
    let secret_access_key = required_encrypted_storage_field(
        "storage.secret_access_key",
        settings.storage_secret_access_key.as_deref(),
        deps,
    )?;
    let region = settings
        .storage_region
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_DEPLOYMENT_S3_REGION)
        .to_string();
    let root_prefix = settings
        .storage_root_prefix
        .as_deref()
        .map(|value| value.trim_matches('/'))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let force_path_style = settings.storage_force_path_style;

    let config = PendingDeploymentStorageConfig {
        bucket: bucket.to_string(),
        endpoint: endpoint.to_string(),
        region,
        root_prefix,
        force_path_style,
        access_key_id,
        secret_access_key,
    };
    let client = build_s3_client(&config).await;

    Ok(ResolvedDeploymentStorage {
        client,
        provider: DeploymentStorageProvider::S3,
        bucket: config.bucket.clone(),
        endpoint: Some(config.endpoint.clone()),
        region: config.region.clone(),
        root_prefix: config.root_prefix.clone(),
        force_path_style: config.force_path_style,
        access_key_id: Some(config.access_key_id.clone()),
        secret_access_key: Some(config.secret_access_key.clone()),
        vector_store_initialized: settings.vector_store_initialized_at.is_some(),
    })
}

pub async fn test_deployment_storage_connection(
    config: &PendingDeploymentStorageConfig,
    probe_key: &str,
    probe_body: &[u8],
) -> Result<(), AppError> {
    let client = build_s3_client(config).await;
    let object_key = config.object_key(probe_key);

    client
        .put_object()
        .bucket(&config.bucket)
        .key(&object_key)
        .body(ByteStream::from(probe_body.to_vec()))
        .send()
        .await
        .map_err(|e| AppError::Validation(format!("storage put_object probe failed: {}", e)))?;

    let downloaded = client
        .get_object()
        .bucket(&config.bucket)
        .key(&object_key)
        .send()
        .await
        .map_err(|e| AppError::Validation(format!("storage get_object probe failed: {}", e)))?;

    let downloaded_bytes = downloaded
        .body
        .collect()
        .await
        .map_err(|e| AppError::Validation(format!("storage read probe failed: {}", e)))?
        .into_bytes();

    if downloaded_bytes.as_ref() != probe_body {
        let _ = client
            .delete_object()
            .bucket(&config.bucket)
            .key(&object_key)
            .send()
            .await;

        return Err(AppError::Validation(
            "storage probe read-back content did not match the uploaded content".to_string(),
        ));
    }

    client
        .delete_object()
        .bucket(&config.bucket)
        .key(&object_key)
        .send()
        .await
        .map_err(|e| AppError::Validation(format!("storage delete_object probe failed: {}", e)))?;

    Ok(())
}

async fn build_s3_client(config: &PendingDeploymentStorageConfig) -> S3Client {
    let shared_config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(config.region.clone()))
        .credentials_provider(Credentials::new(
            config.access_key_id.clone(),
            config.secret_access_key.clone(),
            None,
            None,
            "DeploymentStorage",
        ))
        .load()
        .await;

    let service_config = S3ConfigBuilder::from(&shared_config)
        .endpoint_url(&config.endpoint)
        .force_path_style(config.force_path_style)
        .build();

    S3Client::from_conf(service_config)
}

fn required_storage_field<'a>(
    field_name: &str,
    value: Option<&'a str>,
) -> Result<&'a str, AppError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::Validation(format!("{field_name} is required for deployment storage"))
        })
}

fn required_encrypted_storage_field<D>(
    field_name: &str,
    encrypted_value: Option<&str>,
    deps: &D,
) -> Result<String, AppError>
where
    D: HasEncryptionProvider + ?Sized,
{
    let encrypted = required_storage_field(field_name, encrypted_value)?;
    deps.encryption_provider()
        .decrypt(encrypted)
        .map_err(|error| AppError::Internal(format!("Failed to decrypt {field_name}: {error}")))
}
