use chrono::Utc;
use handlebars::Handlebars;
use serde_json::Value;
use std::sync::LazyLock;

pub mod agent_templates;
pub mod helpers;
pub mod prompt_loader;

pub static HANDLEBARS: LazyLock<Handlebars<'static>> = LazyLock::new(|| {
    let mut hb = Handlebars::new();

    helpers::register_all_helpers(&mut hb);
    agent_templates::register_all_templates(&mut hb);

    hb
});

pub struct AgentTemplates;

impl AgentTemplates {
    // Core decision templates
    pub const STEP_DECISION: &'static str = "step_decision_prompt";
    pub const DEEP_REASONING: &'static str = "deep_reasoning_prompt";
    pub const VALIDATION: &'static str = "validation_prompt";
    pub const SUMMARY: &'static str = "summary_prompt";
    pub const USER_INPUT_REQUEST: &'static str = "user_input_request_prompt";
    pub const EXECUTION_SUMMARY: &'static str = "execution_summary_prompt";
    
    // Tool & context templates
    pub const PARAMETER_GENERATION: &'static str = "parameter_generation_prompt";
    pub const CONTEXT_SEARCH_DERIVATION: &'static str = "context_search_derivation_prompt";
    
    // Workflow node templates
    pub const SWITCH_CASE_EVALUATION: &'static str = "switch_case_evaluation_prompt";
    pub const TRIGGER_EVALUATION: &'static str = "trigger_evaluation_prompt";
    
    // Memory templates
    pub const MEMORY_CONSOLIDATION: &'static str = "memory_consolidation_prompt";
}

pub fn render_template_with_prompt(
    template_name: &str,
    mut context: Value,
) -> Result<String, handlebars::RenderError> {
    // Inject current UTC datetime into all templates
    if let Some(obj) = context.as_object_mut() {
        let now = Utc::now();
        obj.insert(
            "current_datetime_utc".to_string(),
            Value::String(now.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
        );
    }

    let system_prompt = match template_name {
        AgentTemplates::STEP_DECISION => prompt_loader::get_prompt("step_decision_system"),
        AgentTemplates::DEEP_REASONING => prompt_loader::get_prompt("deep_reasoning_system"),
        AgentTemplates::VALIDATION => prompt_loader::get_prompt("validation_system"),
        AgentTemplates::SUMMARY => prompt_loader::get_prompt("summary_system"),
        AgentTemplates::USER_INPUT_REQUEST => {
            prompt_loader::get_prompt("user_input_request_system")
        }
        AgentTemplates::EXECUTION_SUMMARY => prompt_loader::get_prompt("execution_summary_system"),
        AgentTemplates::PARAMETER_GENERATION => {
            prompt_loader::get_prompt("parameter_generation_system")
        }
        AgentTemplates::CONTEXT_SEARCH_DERIVATION => {
            prompt_loader::get_prompt("context_search_derivation_system")
        }
        AgentTemplates::SWITCH_CASE_EVALUATION => {
            prompt_loader::get_prompt("switch_case_evaluation_system")
        }
        AgentTemplates::TRIGGER_EVALUATION => {
            prompt_loader::get_prompt("trigger_evaluation_system")
        }
        AgentTemplates::MEMORY_CONSOLIDATION => {
            prompt_loader::get_prompt("memory_consolidation_system")
        }
        _ => None,
    };

    // If we have a system prompt, render it first then inject it into the context
    if let Some(prompt_template) = system_prompt {
        // Render the system prompt with the current context using the global HANDLEBARS
        let mut rendered_prompt = HANDLEBARS
            .render_template(prompt_template, &context)
            .unwrap();

        // Append custom system instructions if provided in the context
        if let Some(custom_instructions) = context.get("custom_system_instructions") {
            if let Some(custom_str) = custom_instructions.as_str() {
                rendered_prompt.push_str("\n\n");
                rendered_prompt.push_str(custom_str);
            }
        }

        if let Some(obj) = context.as_object_mut() {
            obj.insert("system_prompt".to_string(), Value::String(rendered_prompt));
        }
    }

    HANDLEBARS.render(template_name, &context)
}
