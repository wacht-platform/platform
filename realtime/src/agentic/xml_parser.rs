use serde::de::DeserializeOwned;
use shared::error::AppError;

pub fn from_str<T: DeserializeOwned>(s: &str) -> Result<T, AppError> {
    let first_brace = s.find('<');
    let last_brace = s.rfind('>');

    let xml_str = match (first_brace, last_brace) {
        (Some(start), Some(end)) if end > start => &s[start..=end],
        _ => s,
    };

    quick_xml::de::from_str(xml_str).map_err(|e| {
        eprintln!("Failed to parse XML:\n{}\nError: {}", xml_str, e);
        AppError::Internal(format!("Failed to parse XML response: {}", e))
    })
}
