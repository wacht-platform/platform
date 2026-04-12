use serde_json::json;

pub(crate) fn string_enum(values: &[&str]) -> Option<Vec<serde_json::Value>> {
    Some(values.iter().map(|value| json!(value)).collect())
}
