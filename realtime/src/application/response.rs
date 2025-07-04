use axum::http::StatusCode;
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
