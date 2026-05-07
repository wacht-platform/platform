//! Public listing of the runtime's built-in tools (name, description, schema).
//! Used by the console at hook/tool config time so operators can see what
//! internal tools exist and what args they accept.

use serde_json::Value;

use crate::executor::tools::definitions::internal_tools;
use models::SchemaField;

#[derive(Debug, Clone)]
pub struct InternalToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub fn list_internal_tool_specs() -> Vec<InternalToolSpec> {
    internal_tools()
        .into_iter()
        .map(|(name, description, _kind, fields)| InternalToolSpec {
            name: name.to_string(),
            description: description.to_string(),
            input_schema: SchemaField::object_json_schema(&fields),
        })
        .collect()
}
