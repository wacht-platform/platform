use std::marker::PhantomData;

use async_nats::{Client as NatsClient, jetstream::Context as NatsJetStream};
use aws_sdk_s3::Client as S3Client;
use redis::Client as RedisClient;

use crate::{
    HasAgentStorageClient, HasClickHouseService, HasCloudflareService, HasDbRouter,
    HasDnsVerificationService, HasEncryptionService, HasIdGenerator, HasNatsClient,
    HasNatsJetStream, HasPostmarkService, HasRedis, HasS3Client, HasTemplateRenderer,
    HasTextProcessingService, HasEmbeddingProvider,
    clickhouse::ClickHouseService,
    cloudflare::CloudflareService,
    db_router::DbRouter,
    dns_verification::DnsVerificationService,
    embedding::EmbeddingProvider,
    encryption::EncryptionService,
    error::AppError,
    postmark::PostmarkService,
    state::AppState,
    text_processing::TextProcessingService,
};

pub struct Missing;
pub struct Present;

pub struct AppDeps<
    'a,
    Db = Missing,
    Redis = Missing,
    Enc = Missing,
    Cf = Missing,
    Pm = Missing,
    Dns = Missing,
    Nats = Missing,
    S3 = Missing,
    Id = Missing,
    Tpl = Missing,
> {
    app: &'a AppState,
    _marker: PhantomData<(Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl)>,
}

pub fn from_app(app: &AppState) -> AppDeps<'_> {
    AppDeps {
        app,
        _marker: PhantomData,
    }
}

impl<'a, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl>
    AppDeps<'a, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl>
{
    pub fn db(self) -> AppDeps<'a, Present, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn redis(self) -> AppDeps<'a, Db, Present, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn enc(self) -> AppDeps<'a, Db, Redis, Present, Cf, Pm, Dns, Nats, S3, Id, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn cloudflare(self) -> AppDeps<'a, Db, Redis, Enc, Present, Pm, Dns, Nats, S3, Id, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn postmark(self) -> AppDeps<'a, Db, Redis, Enc, Cf, Present, Dns, Nats, S3, Id, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn dns(self) -> AppDeps<'a, Db, Redis, Enc, Cf, Pm, Present, Nats, S3, Id, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn nats(self) -> AppDeps<'a, Db, Redis, Enc, Cf, Pm, Dns, Present, S3, Id, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn s3(self) -> AppDeps<'a, Db, Redis, Enc, Cf, Pm, Dns, Nats, Present, Id, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn id(self) -> AppDeps<'a, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Present, Tpl> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }

    pub fn template(self) -> AppDeps<'a, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Present> {
        AppDeps {
            app: self.app,
            _marker: PhantomData,
        }
    }
}

impl<Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl> HasDbRouter
    for AppDeps<'_, Present, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl>
{
    fn db_router(&self) -> &DbRouter {
        &self.app.db_router
    }
}

impl<Db, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl> HasRedis
    for AppDeps<'_, Db, Present, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl>
{
    fn redis_client(&self) -> &RedisClient {
        &self.app.redis_client
    }
}

impl<Db, Redis, Cf, Pm, Dns, Nats, S3, Id, Tpl> HasEncryptionService
    for AppDeps<'_, Db, Redis, Present, Cf, Pm, Dns, Nats, S3, Id, Tpl>
{
    fn encryption_service(&self) -> &EncryptionService {
        &self.app.encryption_service
    }
}

impl<Db, Redis, Enc, Pm, Dns, Nats, S3, Id, Tpl> HasCloudflareService
    for AppDeps<'_, Db, Redis, Enc, Present, Pm, Dns, Nats, S3, Id, Tpl>
{
    fn cloudflare_service(&self) -> &CloudflareService {
        &self.app.cloudflare_service
    }
}

impl<Db, Redis, Enc, Cf, Dns, Nats, S3, Id, Tpl> HasPostmarkService
    for AppDeps<'_, Db, Redis, Enc, Cf, Present, Dns, Nats, S3, Id, Tpl>
{
    fn postmark_service(&self) -> &PostmarkService {
        &self.app.postmark_service
    }
}

impl<Db, Redis, Enc, Cf, Pm, Nats, S3, Id, Tpl> HasDnsVerificationService
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Present, Nats, S3, Id, Tpl>
{
    fn dns_verification_service(&self) -> &DnsVerificationService {
        &self.app.dns_verification_service
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id> HasTemplateRenderer
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Present>
{
    fn render_template(
        &self,
        template: &str,
        variables: &serde_json::Value,
    ) -> Result<String, AppError> {
        self.app
            .handlebars
            .render_template(template, variables)
            .map_err(|e| AppError::BadRequest(format!("Failed to render template: {}", e)))
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Tpl> HasIdGenerator
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Present, Tpl>
{
    fn id_generator(&self) -> &sonyflake::Sonyflake {
        &self.app.sf
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, S3, Id, Tpl> HasNatsClient
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Present, S3, Id, Tpl>
{
    fn nats_client(&self) -> &NatsClient {
        &self.app.nats_client
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, S3, Id, Tpl> HasNatsJetStream
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Present, S3, Id, Tpl>
{
    fn nats_jetstream(&self) -> &NatsJetStream {
        &self.app.nats_jetstream
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, Nats, Id, Tpl> HasS3Client
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Nats, Present, Id, Tpl>
{
    fn s3_client(&self) -> &S3Client {
        &self.app.s3_client
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl> HasAgentStorageClient
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl>
{
    fn agent_storage_client(&self) -> Result<&S3Client, AppError> {
        self.app
            .agent_storage_client
            .as_ref()
            .ok_or_else(|| AppError::Internal("Agent storage client not configured".to_string()))
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl> HasClickHouseService
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl>
{
    fn clickhouse_service(&self) -> &ClickHouseService {
        &self.app.clickhouse_service
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl> HasTextProcessingService
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl>
{
    fn text_processing_service(&self) -> &TextProcessingService {
        &self.app.text_processing_service
    }
}

impl<Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl> HasEmbeddingProvider
    for AppDeps<'_, Db, Redis, Enc, Cf, Pm, Dns, Nats, S3, Id, Tpl>
{
    fn embedding_provider(&self) -> &EmbeddingProvider {
        &self.app.embedding_provider
    }
}
