use serde_json::Value;

pub(super) fn infer_schema_hint(value: &Value) -> String {
    infer_schema_recursive(value, 0)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ComplexityMetrics {
    pub max_depth: usize,
    pub leaf_count: usize,
    pub max_object_array_len: usize,
}

pub(super) fn complexity_metrics(value: &Value) -> ComplexityMetrics {
    let mut metrics = ComplexityMetrics {
        max_depth: 0,
        leaf_count: 0,
        max_object_array_len: 0,
    };
    walk_metrics(value, 0, &mut metrics);
    metrics
}

fn walk_metrics(value: &Value, depth: usize, metrics: &mut ComplexityMetrics) {
    if depth > metrics.max_depth {
        metrics.max_depth = depth;
    }
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                metrics.leaf_count += 1;
                return;
            }
            for v in map.values() {
                walk_metrics(v, depth + 1, metrics);
            }
        }
        Value::Array(arr) => {
            let object_count = arr.iter().filter(|v| v.is_object()).count();
            if object_count > metrics.max_object_array_len {
                metrics.max_object_array_len = object_count;
            }
            if arr.is_empty() {
                metrics.leaf_count += 1;
                return;
            }
            for v in arr {
                walk_metrics(v, depth + 1, metrics);
            }
        }
        _ => metrics.leaf_count += 1,
    }
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
