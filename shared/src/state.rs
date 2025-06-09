use std::str::FromStr;

use aws_config::Region;
use aws_sdk_s3::Client as S3Client;

use redis::Client as RedisClient;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{
    services::{
        ClickHouseService, CloudflareService, DnsVerificationService, EmbeddingService,
        PostmarkService, TextProcessingService,
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
    pub embedding_service: EmbeddingService,
    pub text_processing_service: TextProcessingService,
    pub clickhouse_service: ClickHouseService,
}

impl AppState {
    pub async fn new_from_env() -> Self {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        let r2_endpoint_url =
            std::env::var("R2_ENDPOINT_URL").expect("R2_ENDPOINT_URL must be set");
        let r2_access_key_id =
            std::env::var("R2_ACCESS_KEY_ID").expect("R2_ACCESS_KEY_ID must be set");
        let r2_secret_access_key =
            std::env::var("R2_SECRET_ACCESS_KEY").expect("R2_SECRET_ACCESS_KEY must be set");

        let s3_client = S3Client::new(
            &aws_config::from_env()
                .endpoint_url(r2_endpoint_url)
                .credentials_provider(aws_sdk_s3::config::Credentials::new(
                    r2_access_key_id,
                    r2_secret_access_key,
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
            .finalize()
            .expect("Failed to create Sonyflake");

        let redis_client =
            RedisClient::open(std::env::var("REDIS_URL").expect("REDIS_URL must be set"))
                .expect("Failed to create Redis client");

        let mut handlebars = handlebars::Handlebars::new();

        handlebars.register_helper("image", Box::new(handlebars_helpers::ImageHelper));

        let cloudflare_api_key =
            std::env::var("CLOUDFLARE_API_KEY").expect("CLOUDFLARE_API_KEY must be set");
        let cloudflare_zone_id =
            std::env::var("CLOUDFLARE_ZONE_ID").expect("CLOUDFLARE_ZONE_ID must be set");
        let cloudflare_service = CloudflareService::new(cloudflare_api_key, cloudflare_zone_id);

        let postmark_account_token =
            std::env::var("POSTMARK_ACCOUNT_TOKEN").expect("POSTMARK_ACCOUNT_TOKEN must be set");
        let postmark_server_token =
            std::env::var("POSTMARK_SERVER_TOKEN").expect("POSTMARK_SERVER_TOKEN must be set");
        let postmark_service = PostmarkService::new(postmark_account_token, postmark_server_token);

        let dns_verification_service = DnsVerificationService::new();

        let text_processing_service = TextProcessingService::new();

        let embedding_service =
            EmbeddingService::new().expect("Failed to initialize embedding service");

        let clickhouse_url =
            std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
        let clickhouse_password =
            std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_else(|_| "".to_string());

        let clickhouse_service = ClickHouseService::new(&clickhouse_url, &clickhouse_password)
            .expect("Failed to initialize ClickHouse service");

        Self {
            db_pool: pool,
            s3_client,
            sf,
            redis_client,
            handlebars,
            cloudflare_service,
            postmark_service,
            dns_verification_service,
            embedding_service,
            text_processing_service,
            clickhouse_service,
        }
    }
}
