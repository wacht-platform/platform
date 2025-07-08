use serde::de::DeserializeOwned;
use shared::error::AppError;
use tracing::debug;

pub fn from_str<T: DeserializeOwned>(s: &str) -> Result<T, AppError> {
    let json_str = s.trim();
    
    debug!("Parsing JSON: {}", json_str);

    serde_json::from_str(json_str).map_err(|e| {
        eprintln!("Failed to parse JSON:\n{}\nError: {}", json_str, e);
        AppError::Internal(format!("Failed to parse JSON response: {}", e))
    })
}