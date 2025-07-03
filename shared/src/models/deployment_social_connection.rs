use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgTypeInfo;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SocialConnectionProvider {
    XOauth,
    GithubOauth,
    GitlabOauth,
    GoogleOauth,
    FacebookOauth,
    MicrosoftOauth,
    LinkedinOauth,
    DiscordOauth,
    AppleOauth,
}

impl FromStr for SocialConnectionProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "x_oauth" => Ok(SocialConnectionProvider::XOauth),
            "github_oauth" => Ok(SocialConnectionProvider::GithubOauth),
            "gitlab_oauth" => Ok(SocialConnectionProvider::GitlabOauth),
            "google_oauth" => Ok(SocialConnectionProvider::GoogleOauth),
            "facebook_oauth" => Ok(SocialConnectionProvider::FacebookOauth),
            "microsoft_oauth" => Ok(SocialConnectionProvider::MicrosoftOauth),
            "linkedin_oauth" => Ok(SocialConnectionProvider::LinkedinOauth),
            "discord_oauth" => Ok(SocialConnectionProvider::DiscordOauth),
            "apple_oauth" => Ok(SocialConnectionProvider::AppleOauth),
            _ => Err(format!("Invalid social connection provider: {}", s)),
        }
    }
}

impl From<SocialConnectionProvider> for String {
    fn from(provider: SocialConnectionProvider) -> Self {
        match provider {
            SocialConnectionProvider::XOauth => "x_oauth".to_string(),
            SocialConnectionProvider::GithubOauth => "github_oauth".to_string(),
            SocialConnectionProvider::GitlabOauth => "gitlab_oauth".to_string(),
            SocialConnectionProvider::GoogleOauth => "google_oauth".to_string(),
            SocialConnectionProvider::FacebookOauth => "facebook_oauth".to_string(),
            SocialConnectionProvider::MicrosoftOauth => "microsoft_oauth".to_string(),
            SocialConnectionProvider::LinkedinOauth => "linkedin_oauth".to_string(),
            SocialConnectionProvider::DiscordOauth => "discord_oauth".to_string(),
            SocialConnectionProvider::AppleOauth => "apple_oauth".to_string(),
        }
    }
}

// Implement sqlx::Type and sqlx::Decode for SocialConnectionProvider
impl sqlx::Type<sqlx::Postgres> for SocialConnectionProvider {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SocialConnectionProvider {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        SocialConnectionProvider::from_str(value).map_err(|e| e.into())
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct OauthCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentSocialConnection {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deployment_id: Option<i64>,
    pub provider: Option<SocialConnectionProvider>,
    pub enabled: bool,
    pub credentials: Option<OauthCredentials>,
}
