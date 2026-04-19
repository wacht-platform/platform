use serde_json::Value;
use std::collections::HashSet;

fn schema_type_matches(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(type_name) => type_name == expected,
        Value::Array(items) => items.iter().any(|item| item.as_str() == Some(expected)),
        _ => false,
    }
}

pub(crate) fn normalize_json_schema(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut normalized: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(key, value)| {
                    let value = if key == "type" {
                        match value {
                            Value::String(type_name) => Value::String(
                                match type_name.as_str() {
                                    "OBJECT" => "object",
                                    "ARRAY" => "array",
                                    "STRING" => "string",
                                    "INTEGER" => "integer",
                                    "NUMBER" => "number",
                                    "BOOLEAN" => "boolean",
                                    other => other,
                                }
                                .to_string(),
                            ),
                            other => normalize_json_schema(other),
                        }
                    } else {
                        normalize_json_schema(value)
                    };
                    (key, value)
                })
                .collect();

            let is_object = normalized
                .get("type")
                .map(|value| schema_type_matches(value, "object"))
                .unwrap_or(false);

            if is_object {
                normalized
                    .entry("additionalProperties".to_string())
                    .or_insert(Value::Bool(false));
            }

            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_json_schema).collect()),
        other => other,
    }
}

pub(crate) fn normalize_openai_tool_schema(value: Value) -> Value {
    normalize_openai_tool_schema_node(value)
}

pub(crate) fn normalize_openai_response_schema(value: Value) -> Value {
    normalize_openai_tool_schema_node(value)
}

fn normalize_openai_tool_schema_node(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut normalized = serde_json::Map::new();
            let mut original_required = HashSet::new();

            if let Some(Value::Array(required)) = map.get("required") {
                for item in required {
                    if let Some(name) = item.as_str() {
                        original_required.insert(name.to_string());
                    }
                }
            }

            for (key, value) in map {
                if key == "properties" {
                    let properties = match value {
                        Value::Object(properties) => {
                            let mut normalized_properties = serde_json::Map::new();
                            for (property_name, property_schema) in properties {
                                let mut property_schema =
                                    normalize_openai_tool_schema_node(property_schema);
                                if !original_required.contains(&property_name) {
                                    property_schema = make_openai_optional_schema(property_schema);
                                }
                                normalized_properties.insert(property_name, property_schema);
                            }
                            Value::Object(normalized_properties)
                        }
                        other => normalize_openai_tool_schema_node(other),
                    };
                    normalized.insert(key, properties);
                    continue;
                }

                if key == "required" {
                    continue;
                }

                let value = if key == "type" {
                    match value {
                        Value::String(type_name) => Value::String(
                            match type_name.as_str() {
                                "OBJECT" => "object",
                                "ARRAY" => "array",
                                "STRING" => "string",
                                "INTEGER" => "integer",
                                "NUMBER" => "number",
                                "BOOLEAN" => "boolean",
                                other => other,
                            }
                            .to_string(),
                        ),
                        other => normalize_openai_tool_schema_node(other),
                    }
                } else {
                    normalize_openai_tool_schema_node(value)
                };

                normalized.insert(key, value);
            }

            let is_object = normalized
                .get("type")
                .map(|value| schema_type_matches(value, "object"))
                .unwrap_or(false);

            if is_object {
                normalized.insert("additionalProperties".to_string(), Value::Bool(false));

                if let Some(Value::Object(properties)) = normalized.get("properties") {
                    let required = properties.keys().cloned().collect::<Vec<_>>();
                    if !required.is_empty() {
                        normalized.insert(
                            "required".to_string(),
                            Value::Array(required.into_iter().map(Value::String).collect()),
                        );
                    }
                }
            }

            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(normalize_openai_tool_schema_node)
                .collect(),
        ),
        other => other,
    }
}

fn make_openai_optional_schema(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            match map.remove("type") {
                Some(Value::String(type_name)) => {
                    map.insert(
                        "type".to_string(),
                        Value::Array(vec![
                            Value::String(type_name),
                            Value::String("null".to_string()),
                        ]),
                    );
                }
                Some(Value::Array(mut items)) => {
                    let has_null = items.iter().any(|item| item.as_str() == Some("null"));
                    if !has_null {
                        items.push(Value::String("null".to_string()));
                    }
                    map.insert("type".to_string(), Value::Array(items));
                }
                Some(other) => {
                    map.insert("type".to_string(), other);
                }
                None => {}
            }

            if let Some(Value::Array(enum_values)) = map.get_mut("enum") {
                if !enum_values.iter().any(Value::is_null) {
                    enum_values.push(Value::Null);
                }
            }

            Value::Object(map)
        }
        other => other,
    }
}

pub(crate) fn normalize_gemini_function_schema(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut normalized = serde_json::Map::new();

            for (key, value) in map {
                if key == "properties" {
                    let property_map = match value {
                        Value::Object(properties) => Value::Object(
                            properties
                                .into_iter()
                                .map(|(property_name, property_schema)| {
                                    (
                                        property_name,
                                        normalize_gemini_function_schema(property_schema),
                                    )
                                })
                                .collect(),
                        ),
                        other => normalize_gemini_function_schema(other),
                    };
                    normalized.insert(key, property_map);
                    continue;
                }

                let normalized_value = if key == "type" {
                    match value {
                        Value::String(type_name) => Value::String(
                            match type_name.as_str() {
                                "OBJECT" => "object",
                                "ARRAY" => "array",
                                "STRING" => "string",
                                "INTEGER" => "integer",
                                "NUMBER" => "number",
                                "BOOLEAN" => "boolean",
                                other => other,
                            }
                            .to_string(),
                        ),
                        other => normalize_gemini_function_schema(other),
                    }
                } else {
                    normalize_gemini_function_schema(value)
                };

                if matches!(key.as_str(), "additionalProperties" | "title") {
                    continue;
                }

                normalized.insert(key, normalized_value);
            }

            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(normalize_gemini_function_schema)
                .collect(),
        ),
        other => other,
    }
}
