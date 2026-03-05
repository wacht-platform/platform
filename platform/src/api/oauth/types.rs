use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthAppPathParams {
    pub(crate) oauth_app_slug: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthClientPathParams {
    pub(crate) oauth_app_slug: String,
    pub(crate) oauth_client_id: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthGrantPathParams {
    pub(crate) oauth_app_slug: String,
    pub(crate) oauth_client_id: i64,
    pub(crate) grant_id: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthScopePathParams {
    pub(crate) oauth_app_slug: String,
    pub(crate) scope: String,
}
