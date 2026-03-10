use serde::de::DeserializeOwned;

pub fn json_default<T: DeserializeOwned + Default>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap_or_default()
}

pub fn json_optional<T: DeserializeOwned>(value: Option<serde_json::Value>) -> Option<T> {
    value.and_then(|v| serde_json::from_value(v).ok())
}

pub fn json_option_default(
    value: Option<serde_json::Value>,
    default: serde_json::Value,
) -> serde_json::Value {
    value.unwrap_or(default)
}

pub fn json_vec_default(value: serde_json::Value) -> Vec<String> {
    if value.is_null() {
        Vec::new()
    } else {
        json_default(value)
    }
}
