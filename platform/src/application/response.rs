use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct ApiError {
    pub message: String,
    pub code: u16,
}

#[derive(Clone, Serialize)]
pub struct ApiErrorResponse {
    #[serde(skip_serializing)]
    pub staus_code: StatusCode,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ApiError>,
}

#[derive(Clone, Serialize)]
pub struct ApiSuccess<T> {
    #[serde(flatten)]
    pub data: T,
    #[serde(skip_serializing)]
    pub status: StatusCode,
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> axum::response::Response {
        (self.staus_code, Json(self)).into_response()
    }
}

impl<T> IntoResponse for ApiSuccess<T>
where
    T: Serialize,
{
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self)).into_response()
    }
}

impl<T> From<T> for ApiSuccess<T> {
    fn from(data: T) -> Self {
        ApiSuccess {
            data,
            status: StatusCode::OK,
        }
    }
}

impl<T> From<(StatusCode, T)> for ApiSuccess<T> {
    fn from((status, data): (StatusCode, T)) -> Self {
        ApiSuccess { data, status }
    }
}

impl From<(StatusCode, Vec<ApiError>)> for ApiErrorResponse {
    fn from(value: (StatusCode, Vec<ApiError>)) -> Self {
        ApiErrorResponse {
            staus_code: value.0,
            errors: value.1,
        }
    }
}

impl From<(StatusCode, ApiError)> for ApiErrorResponse {
    fn from(value: (StatusCode, ApiError)) -> Self {
        ApiErrorResponse {
            staus_code: value.0,
            errors: vec![value.1],
        }
    }
}

impl From<(StatusCode, String)> for ApiErrorResponse {
    fn from(value: (StatusCode, String)) -> Self {
        ApiErrorResponse {
            staus_code: value.0,
            errors: vec![ApiError {
                message: value.1,
                code: u16::from(value.0),
            }],
        }
    }
}

impl From<(StatusCode, &str)> for ApiErrorResponse {
    fn from(value: (StatusCode, &str)) -> Self {
        ApiErrorResponse {
            staus_code: value.0,
            errors: vec![ApiError {
                message: value.1.to_string(),
                code: u16::from(value.0),
            }],
        }
    }
}

pub type ApiResult<T> = Result<ApiSuccess<T>, ApiErrorResponse>;

#[derive(Clone, Serialize)]
pub struct PaginatedResponse<T>
where
    T: Serialize,
{
    pub data: Vec<T>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i32>,
}

impl<T> From<Vec<T>> for PaginatedResponse<T>
where
    T: Serialize,
{
    fn from(data: Vec<T>) -> Self {
        PaginatedResponse {
            data,
            has_more: false,
            limit: None,
            offset: None,
        }
    }
}
