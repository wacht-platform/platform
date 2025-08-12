use axum::{extract::FromRequestParts, http::{request::Parts, StatusCode}};

#[derive(Debug, Clone)]
pub struct ApiKeyContext {
    pub key_id: i64,
    pub app_id: i64,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RequireApiKey(pub ApiKeyContext);

impl<S> FromRequestParts<S> for RequireApiKey
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ApiKeyContext>()
            .cloned()
            .map(RequireApiKey)
            .ok_or((StatusCode::UNAUTHORIZED, "API key context not found"))
    }
}