use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};

#[derive(Debug, Clone)]
pub struct ApiKeyContext {
    pub key_id: i64,
    pub app_slug: String,
    pub permissions: Vec<String>,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub organization_membership_id: Option<i64>,
    pub workspace_membership_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RequireApiKey(pub ApiKeyContext);

impl<S> FromRequestParts<S> for RequireApiKey
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ApiKeyContext>()
            .cloned()
            .map(RequireApiKey)
            .ok_or((StatusCode::UNAUTHORIZED, "API key context not found"))
    }
}
