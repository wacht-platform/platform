use std::str::FromStr;

use aws_config::Region;
use aws_sdk_s3::Client as S3Client;

use redis::Client as RedisClient;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::env::var as env;
use std::error::Error;

use crate::{
    services::{
        ClickHouseService, CloudflareService, DnsVerificationService, PostmarkService,
        TextProcessingService,
    },
    utils::handlebars_helpers,
};

#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub s3_client: S3Client,
    pub sf: sonyflake::Sonyflake,
    pub redis_client: RedisClient,
    pub handlebars: handlebars::Handlebars<'static>,
    pub cloudflare_service: CloudflareService,
    pub postmark_service: PostmarkService,
    pub dns_verification_service: DnsVerificationService,
    pub text_processing_service: TextProcessingService,
    pub clickhouse_service: ClickHouseService,
}

impl AppState {
    pub async fn new_from_env() -> Result<Self, Box<dyn Error>> {
        let database_url = env("DATABASE_URL")?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await?;

        let s3_client = S3Client::new(
            &aws_config::from_env()
                .endpoint_url(env("R2_ENDPOINT_URL")?)
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
            .start_time(
                chrono::DateTime::<chrono::Utc>::from_str("2025-01-01 00:00:00+0000").unwrap(),
            )
            .finalize()?;
        let redis_client = RedisClient::open(env("REDIS_URL")?)?;

        let mut handlebars = handlebars::Handlebars::new();
        handlebars.register_helper("image", Box::new(handlebars_helpers::ImageHelper));

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

        // Initialize ClickHouse tables
        clickhouse_service.init_tables().await?;

        Ok(Self {
            db_pool: pool,
            s3_client,
            sf,
            redis_client,
            handlebars,
            cloudflare_service,
            postmark_service,
            dns_verification_service,
            text_processing_service,
            clickhouse_service,
        })
    }
}
