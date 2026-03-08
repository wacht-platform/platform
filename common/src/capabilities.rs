use async_nats::{Client as NatsClient, jetstream::Context as NatsJetStream};
use aws_sdk_s3::Client as S3Client;
use redis::Client as RedisClient;
use sqlx::PgPool;

use crate::{
    clickhouse::ClickHouseService,
    cloudflare::CloudflareService,
    db_router::{DbRouter, ReadConsistency},
    dns_verification::DnsVerificationService,
    embedding::EmbeddingProvider,
    encryption::EncryptionService,
    error::AppError,
    postmark::PostmarkService,
    state::AppState,
    text_processing::TextProcessingService,
};

pub trait HasDbRouter {
    fn db_router(&self) -> &DbRouter;

    fn writer_pool(&self) -> &PgPool {
        self.db_router().writer()
    }

    fn reader_pool(&self, consistency: ReadConsistency) -> &PgPool {
        self.db_router().reader(consistency)
    }
}

pub trait HasRedis {
    fn redis_client(&self) -> &RedisClient;
}

pub trait HasNatsClient {
    fn nats_client(&self) -> &NatsClient;
}

pub trait HasNatsJetStream {
    fn nats_jetstream(&self) -> &NatsJetStream;
}

pub trait HasIdGenerator {
    fn id_generator(&self) -> &sonyflake::Sonyflake;
}

pub trait HasEncryptionService {
    fn encryption_service(&self) -> &EncryptionService;
}

pub trait HasPostmarkService {
    fn postmark_service(&self) -> &PostmarkService;
}

pub trait HasClickHouseService {
    fn clickhouse_service(&self) -> &ClickHouseService;
}

pub trait HasCloudflareService {
    fn cloudflare_service(&self) -> &CloudflareService;
}

pub trait HasDnsVerificationService {
    fn dns_verification_service(&self) -> &DnsVerificationService;
}

pub trait HasTemplateRenderer {
    fn render_template(
        &self,
        template: &str,
        variables: &serde_json::Value,
    ) -> Result<String, AppError>;
}

pub trait HasS3Client {
    fn s3_client(&self) -> &S3Client;
}

pub trait HasAgentStorageClient {
    fn agent_storage_client(&self) -> Result<&S3Client, AppError>;
}

pub trait HasTextProcessingService {
    fn text_processing_service(&self) -> &TextProcessingService;
}

pub trait HasEmbeddingProvider {
    fn embedding_provider(&self) -> &EmbeddingProvider;
}

impl HasDbRouter for AppState {
    fn db_router(&self) -> &DbRouter {
        &self.db_router
    }
}

impl HasDbRouter for DbRouter {
    fn db_router(&self) -> &DbRouter {
        self
    }
}

impl HasNatsClient for NatsClient {
    fn nats_client(&self) -> &NatsClient {
        self
    }
}

impl HasRedis for AppState {
    fn redis_client(&self) -> &RedisClient {
        &self.redis_client
    }
}

impl HasRedis for RedisClient {
    fn redis_client(&self) -> &RedisClient {
        self
    }
}

impl HasNatsClient for AppState {
    fn nats_client(&self) -> &NatsClient {
        &self.nats_client
    }
}

impl HasNatsJetStream for AppState {
    fn nats_jetstream(&self) -> &NatsJetStream {
        &self.nats_jetstream
    }
}

impl HasIdGenerator for AppState {
    fn id_generator(&self) -> &sonyflake::Sonyflake {
        &self.sf
    }
}

impl HasEncryptionService for AppState {
    fn encryption_service(&self) -> &EncryptionService {
        &self.encryption_service
    }
}

impl HasPostmarkService for AppState {
    fn postmark_service(&self) -> &PostmarkService {
        &self.postmark_service
    }
}

impl HasClickHouseService for AppState {
    fn clickhouse_service(&self) -> &ClickHouseService {
        &self.clickhouse_service
    }
}

impl HasCloudflareService for AppState {
    fn cloudflare_service(&self) -> &CloudflareService {
        &self.cloudflare_service
    }
}

impl HasDnsVerificationService for AppState {
    fn dns_verification_service(&self) -> &DnsVerificationService {
        &self.dns_verification_service
    }
}

impl HasTemplateRenderer for AppState {
    fn render_template(
        &self,
        template: &str,
        variables: &serde_json::Value,
    ) -> Result<String, AppError> {
        self.handlebars
            .render_template(template, variables)
            .map_err(|e| AppError::BadRequest(format!("Failed to render template: {}", e)))
    }
}

impl HasS3Client for AppState {
    fn s3_client(&self) -> &S3Client {
        &self.s3_client
    }
}

impl HasS3Client for S3Client {
    fn s3_client(&self) -> &S3Client {
        self
    }
}

impl HasAgentStorageClient for AppState {
    fn agent_storage_client(&self) -> Result<&S3Client, AppError> {
        self.agent_storage_client
            .as_ref()
            .ok_or_else(|| AppError::Internal("Agent storage client not configured".to_string()))
    }
}

impl HasTextProcessingService for AppState {
    fn text_processing_service(&self) -> &TextProcessingService {
        &self.text_processing_service
    }
}

impl HasEmbeddingProvider for AppState {
    fn embedding_provider(&self) -> &EmbeddingProvider {
        &self.embedding_provider
    }
}
