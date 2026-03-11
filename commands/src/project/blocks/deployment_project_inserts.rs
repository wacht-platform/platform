use super::*;

mod production_insert;
mod project_insert;
mod staging_insert;

fn required_i64(value: Option<i64>, scope: &str, field: &str) -> Result<i64, AppError> {
    value.ok_or_else(|| AppError::Validation(format!("{} {} is required", scope, field)))
}

fn required_str<'a>(
    value: Option<&'a String>,
    scope: &str,
    field: &str,
) -> Result<&'a str, AppError> {
    value
        .map(String::as_str)
        .ok_or_else(|| AppError::Validation(format!("{} {} is required", scope, field)))
}

fn required_json<'a>(
    value: Option<&'a serde_json::Value>,
    scope: &str,
    field: &str,
) -> Result<&'a serde_json::Value, AppError> {
    value.ok_or_else(|| AppError::Validation(format!("{} {} are required", scope, field)))
}

pub(in crate::project) use production_insert::*;
pub(in crate::project) use project_insert::*;
pub(in crate::project) use staging_insert::*;
