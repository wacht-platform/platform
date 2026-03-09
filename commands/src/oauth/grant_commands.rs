use chrono::{DateTime, Utc};
use common::error::AppError;
use serde::de::DeserializeOwned;

fn json_default<T: DeserializeOwned + Default>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap_or_default()
}

mod create_grant;
mod revoke_grant;

pub use create_grant::*;
pub use revoke_grant::*;
