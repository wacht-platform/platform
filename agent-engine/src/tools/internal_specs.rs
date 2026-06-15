//! Public listing of the runtime's built-in tools (name, description, schema).
//! Used by the console at hook/tool config time so operators can see what
//! internal tools exist and what args they accept.

use serde_json::Value;

use crate::executor::agent_loop::meta_tools::ask_user_tool;
use crate::executor::tools::definitions::internal_tools;
use models::SchemaField;

#[derive(Debug, Clone)]
pub struct InternalToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub fn list_internal_tool_specs() -> Vec<InternalToolSpec> {
    let mut specs: Vec<InternalToolSpec> = internal_tools()
        .into_iter()
        .map(|(name, description, _kind, fields)| InternalToolSpec {
            name: name.to_string(),
            description: description.to_string(),
            input_schema: SchemaField::object_json_schema(&fields),
        })
        .collect();

    // `ask_user` is a native meta tool (not in the catalog) but is operator-toggleable,
    // so surface it here for the console. Disabling it removes the agent's only channel
    // for asking the user — it must then resolve via context/defaults or complete/abort
    // instead of pausing the thread for input.
    let ask_user = ask_user_tool();
    specs.push(InternalToolSpec {
        name: ask_user.name,
        description: "Ask the user a question (clarification, choice, confirmation). \
            Disable to stop the agent from pausing the thread to wait for user input."
            .to_string(),
        input_schema: ask_user.input_schema,
    });

    specs
}
