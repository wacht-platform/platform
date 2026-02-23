use serde_json::Value;

pub(super) fn infer_schema_hint(value: &Value) -> String {
    infer_schema_recursive(value, 0)
}

fn infer_schema_recursive(value: &Value, depth: usize) -> String {
    if depth > 5 {
        return "...".to_string();
    }

    match value {
        Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }
            let fields: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", k, infer_type_hint(v, depth + 1)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        Value::Array(arr) => {
            if let Some(first) = arr.first() {
                format!("{}[]", infer_type_hint(first, depth + 1))
            } else {
                "[]".to_string()
            }
        }
        _ => infer_type_hint(value, depth),
    }
}

fn infer_type_hint(value: &Value, depth: usize) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(n) => {
            if n.is_i64() {
                "int".to_string()
            } else {
                "number".to_string()
            }
        }
        Value::String(s) => {
            if s.contains('T') && s.contains(':') && s.len() > 15 {
                "datetime".to_string()
            } else if s.starts_with("http") {
                "url".to_string()
            } else {
                "string".to_string()
            }
        }
        Value::Array(_) | Value::Object(_) => infer_schema_recursive(value, depth),
    }
}
