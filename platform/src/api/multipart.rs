use std::borrow::Cow;

use axum::extract::Multipart;
use axum::http::StatusCode;

use crate::application::response::ApiErrorResponse;

#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

impl MultipartField {
    pub fn text_trimmed(&self) -> Result<String, ApiErrorResponse> {
        let text = String::from_utf8(self.bytes.clone()).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("Multipart field '{}' must be valid UTF-8 text", self.name),
            )
        })?;
        Ok(text.trim().to_string())
    }

    pub fn content_type_or<'a>(&'a self, default: &'a str) -> Cow<'a, str> {
        match &self.content_type {
            Some(v) => Cow::Borrowed(v),
            None => Cow::Borrowed(default),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MultipartPayload {
    fields: Vec<MultipartField>,
}

impl MultipartPayload {
    pub async fn parse(mut multipart: Multipart) -> Result<Self, ApiErrorResponse> {
        let mut fields = Vec::new();

        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
        {
            let name = field
                .name()
                .ok_or_else(|| (StatusCode::BAD_REQUEST, "Multipart field name is missing"))?
                .to_string();
            let content_type = field.content_type().map(ToOwned::to_owned);
            let bytes = field
                .bytes()
                .await
                .map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Invalid multipart field '{}': {}", name, e),
                    )
                })?
                .to_vec();

            fields.push(MultipartField {
                name,
                content_type,
                bytes,
            });
        }

        Ok(Self { fields })
    }

    pub fn fields(&self) -> &[MultipartField] {
        &self.fields
    }

    pub fn required_text(&self, name: &str) -> Result<String, ApiErrorResponse> {
        let field = self
            .fields
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("'{}' is required", name)))?;
        let value = field.text_trimmed()?;
        if value.is_empty() {
            return Err((StatusCode::BAD_REQUEST, format!("'{}' is required", name)).into());
        }
        Ok(value)
    }

    pub fn repeated_text(&self, name: &str) -> Result<Vec<String>, ApiErrorResponse> {
        self.fields
            .iter()
            .filter(|f| f.name == name)
            .map(MultipartField::text_trimmed)
            .filter_map(|result| match result {
                Ok(v) if !v.is_empty() => Some(Ok(v)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }
}
