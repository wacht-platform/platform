use std::str::FromStr;
use std::time::Duration;

use aws_config::{BehaviorVersion, Region};
use aws_sdk_s3::Client as S3Client;

use async_nats::jetstream::Context as NatsJetStream;
use async_nats::{Client as NatsClient, jetstream};
use redis::Client as RedisClient;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::env::var as env;
use std::error::Error;
use wacht::{WachtClient, WachtConfig};

use crate::{
    ClickHouseService, CloudflareService, DnsVerificationService, EncryptionService,
    PostmarkService, TextProcessingService,
};

#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub s3_client: S3Client,
    pub agent_storage_client: Option<S3Client>,
    pub sf: sonyflake::Sonyflake,
    pub redis_client: RedisClient,
    pub handlebars: handlebars::Handlebars<'static>,
    pub cloudflare_service: CloudflareService,
    pub postmark_service: PostmarkService,
    pub dns_verification_service: DnsVerificationService,
    pub text_processing_service: TextProcessingService,
    pub clickhouse_service: ClickHouseService,
    pub nats_client: NatsClient,
    pub nats_jetstream: NatsJetStream,
    pub encryption_service: EncryptionService,
    pub wacht_client: Option<WachtClient>,
}

impl AppState {
    pub async fn new_from_env() -> Result<Self, Box<dyn Error>> {
        let database_url = if !env("USE_PUBLIC_NETWORK").is_ok() {
            env("DATABASE_PRIMARY_PRIVATE")?
        } else {
            env("DATABASE_PRIMARY_PUBLIC")?
        };
        let pool = PgPoolOptions::new()
            .min_connections(5)
            .acquire_timeout(Duration::from_secs(30))
            .max_lifetime(Some(Duration::from_secs(150)))
            .max_connections(50)
            .connect(&database_url)
            .await?;

        let s3_client = S3Client::new(
            &aws_config::defaults(BehaviorVersion::latest())
                .endpoint_url(env("R2_ENDPOINT")?)
                .credentials_provider(aws_sdk_s3::config::Credentials::new(
                    env("R2_ACCESS_KEY_ID")?,
                    env("R2_SECRET_ACCESS_KEY")?,
                    None,
                    None,
                    "R2",
                ))
                .region(Region::new("auto"))
                .load()
                .await,
        );

        let sf = sonyflake::Sonyflake::builder()
            .start_time(chrono::DateTime::<chrono::Utc>::from_str(
                "2025-01-01 00:00:00+0000",
            )?)
            .machine_id(&|| Ok(rand::random::<u16>()))
            .finalize()?;

        let redis_client = RedisClient::open(env("REDIS_URL")?)?;

        let mut handlebars = handlebars::Handlebars::new();
        handlebars.register_helper("image", Box::new(crate::utils::handlebars::ImageHelper));

        let cloudflare_service =
            CloudflareService::new(env("CLOUDFLARE_API_KEY")?, env("CLOUDFLARE_ZONE_ID")?);
        let postmark_service = PostmarkService::new(
            env("POSTMARK_ACCOUNT_TOKEN")?,
            env("POSTMARK_SERVER_TOKEN")?,
        );

        let dns_verification_service = DnsVerificationService::new();
        let text_processing_service = TextProcessingService::new();

        let clickhouse_service =
            ClickHouseService::new(env("CLICKHOUSE_HOST")?, env("CLICKHOUSE_PASSWORD")?)?;

        let nats_options =
            async_nats::ConnectOptions::new().request_timeout(Some(Duration::from_secs(3000)));
        let nats_client = async_nats::connect_with_options(env("NATS_HOST")?, nats_options).await?;
        let nats_jetstream = jetstream::new(nats_client.clone());

        let encryption_service = EncryptionService::new(&env("ENCRYPTION_KEY")?)?;

        let agent_storage_client = if let Ok(gateway_url) = env("AGENT_STORAGE_GATEWAY_URL") {
            let access_key = env("AGENT_STORAGE_ACCESS_KEY").unwrap();
            let secret_key = env("AGENT_STORAGE_SECRET_KEY").unwrap();

            let client = S3Client::new(
                &aws_config::defaults(BehaviorVersion::latest())
                    .endpoint_url(gateway_url)
                    .credentials_provider(aws_sdk_s3::config::Credentials::new(
                        access_key,
                        secret_key,
                        None,
                        None,
                        "AgentStorage",
                    ))
                    .region(Region::new("us-east-1"))
                    .load()
                    .await,
            );

            Some(client)
        } else {
            None
        };

        let wacht_client = WachtConfig::from_env()
            .ok()
            .and_then(|config| WachtClient::new(config).ok());

        Ok(Self {
            db_pool: pool,
            s3_client,
            agent_storage_client,
            sf,
            redis_client,
            handlebars,
            cloudflare_service,
            postmark_service,
            dns_verification_service,
            text_processing_service,
            clickhouse_service,
            nats_client,
            nats_jetstream,
            encryption_service,
            wacht_client,
        })
    }
}
