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

pub trait HasRedisProvider {
    fn redis_provider(&self) -> &RedisClient;
}

pub trait HasNatsProvider {
    fn nats_provider(&self) -> &NatsClient;
}

pub trait HasNatsJetStreamProvider {
    fn nats_jetstream_provider(&self) -> &NatsJetStream;
}

pub trait HasIdProvider {
    fn id_provider(&self) -> &sonyflake::Sonyflake;
}

pub trait HasEncryptionProvider {
    fn encryption_provider(&self) -> &EncryptionService;
}

pub trait HasPostmarkProvider {
    fn postmark_provider(&self) -> &PostmarkService;
}

pub trait HasClickHouseProvider {
    fn clickhouse_provider(&self) -> &ClickHouseService;
}

pub trait HasCloudflareProvider {
    fn cloudflare_provider(&self) -> &CloudflareService;
}

pub trait HasDnsVerificationProvider {
    fn dns_verification_provider(&self) -> &DnsVerificationService;
}

pub trait HasTemplateRenderer {
    fn render_template(
        &self,
        template: &str,
        variables: &serde_json::Value,
    ) -> Result<String, AppError>;
}

pub trait HasS3Provider {
    fn s3_provider(&self) -> &S3Client;
}

pub trait HasTextProcessingProvider {
    fn text_processing_provider(&self) -> &TextProcessingService;
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

impl HasNatsProvider for NatsClient {
    fn nats_provider(&self) -> &NatsClient {
        self
    }
}

impl HasRedisProvider for AppState {
    fn redis_provider(&self) -> &RedisClient {
        &self.redis_client
    }
}

impl HasRedisProvider for RedisClient {
    fn redis_provider(&self) -> &RedisClient {
        self
    }
}

impl HasNatsProvider for AppState {
    fn nats_provider(&self) -> &NatsClient {
        &self.nats_client
    }
}

impl HasNatsJetStreamProvider for AppState {
    fn nats_jetstream_provider(&self) -> &NatsJetStream {
        &self.nats_jetstream
    }
}

impl HasIdProvider for AppState {
    fn id_provider(&self) -> &sonyflake::Sonyflake {
        &self.sf
    }
}

impl HasEncryptionProvider for AppState {
    fn encryption_provider(&self) -> &EncryptionService {
        &self.encryption_service
    }
}

impl HasPostmarkProvider for AppState {
    fn postmark_provider(&self) -> &PostmarkService {
        &self.postmark_service
    }
}

impl HasClickHouseProvider for AppState {
    fn clickhouse_provider(&self) -> &ClickHouseService {
        &self.clickhouse_service
    }
}

impl HasCloudflareProvider for AppState {
    fn cloudflare_provider(&self) -> &CloudflareService {
        &self.cloudflare_service
    }
}

impl HasDnsVerificationProvider for AppState {
    fn dns_verification_provider(&self) -> &DnsVerificationService {
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

impl HasS3Provider for AppState {
    fn s3_provider(&self) -> &S3Client {
        &self.s3_client
    }
}

impl HasS3Provider for S3Client {
    fn s3_provider(&self) -> &S3Client {
        self
    }
}

impl HasTextProcessingProvider for AppState {
    fn text_processing_provider(&self) -> &TextProcessingService {
        &self.text_processing_service
    }
}

impl HasEmbeddingProvider for AppState {
    fn embedding_provider(&self) -> &EmbeddingProvider {
        &self.embedding_provider
    }
}
