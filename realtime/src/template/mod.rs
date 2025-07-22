use handlebars::Handlebars;
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
    pub const ACKNOWLEDGMENT: &'static str = "acknowledgment_prompt";
    pub const IDEATION: &'static str = "ideation_prompt";
    pub const CONTEXT_GATHERING: &'static str = "context_gathering_prompt";
    pub const TASK_BREAKDOWN: &'static str = "task_breakdown_prompt";
    pub const TASK_EXECUTION: &'static str = "task_execution_prompt";
    pub const VALIDATION: &'static str = "validation_prompt";
    pub const PARAMETER_GENERATION: &'static str = "parameter_generation_prompt";
    pub const WORKFLOW_VALIDATION: &'static str = "workflow_validation_prompt";
    pub const SUMMARY: &'static str = "summary_prompt";
    pub const STEP_DECISION: &'static str = "step_decision_prompt";
    pub const CONTEXT_SEARCH_DERIVATION: &'static str = "context_search_derivation_prompt";
    pub const KNOWLEDGE_BASE_SEARCH: &'static str = "knowledge_base_search_prompt";
    pub const KB_SEARCH_PLAN: &'static str = "kb_search_plan_prompt";
    pub const KB_SEARCH_EXECUTION: &'static str = "kb_search_execution_prompt";
    pub const KB_SEARCH_VALIDATION: &'static str = "kb_search_validation_prompt";
}

pub fn render_template(
    template_name: &str,
    context: &impl serde::Serialize,
) -> Result<String, handlebars::RenderError> {
    HANDLEBARS.render(template_name, context)
}

pub fn render_template_with_prompt(
    template_name: &str,
    context: &impl serde::Serialize,
) -> Result<String, handlebars::RenderError> {
    use serde_json::json;

    // Convert context to JSON value
    let mut context_value = serde_json::to_value(context).map_err(|e| {
        use handlebars::RenderErrorReason;
        handlebars::RenderError::from(RenderErrorReason::NestedError(Box::new(e)))
    })?;

    // Determine which system prompt to use based on template name
    let system_prompt = match template_name {
        AgentTemplates::ACKNOWLEDGMENT => prompt_loader::get_prompt("acknowledgment_system"),
        AgentTemplates::CONTEXT_GATHERING => prompt_loader::get_prompt("context_gathering_system"),
        AgentTemplates::CONTEXT_SEARCH_DERIVATION => {
            prompt_loader::get_prompt("context_search_derivation_system")
        }
        AgentTemplates::IDEATION => prompt_loader::get_prompt("ideation_system"),
        AgentTemplates::PARAMETER_GENERATION => {
            prompt_loader::get_prompt("parameter_generation_system")
        }
        AgentTemplates::STEP_DECISION => prompt_loader::get_prompt("step_decision_system"),
        AgentTemplates::SUMMARY => prompt_loader::get_prompt("summary_system"),
        AgentTemplates::TASK_BREAKDOWN => prompt_loader::get_prompt("task_breakdown_system"),
        AgentTemplates::TASK_EXECUTION => prompt_loader::get_prompt("task_execution_system"),
        AgentTemplates::VALIDATION => prompt_loader::get_prompt("validation_system"),
        AgentTemplates::WORKFLOW_VALIDATION => {
            prompt_loader::get_prompt("workflow_validation_system")
        }
        AgentTemplates::KNOWLEDGE_BASE_SEARCH => {
            prompt_loader::get_prompt("knowledge_base_search_system")
        }
        AgentTemplates::KB_SEARCH_PLAN => {
            prompt_loader::get_prompt("kb_search_plan_system")
        }
        AgentTemplates::KB_SEARCH_EXECUTION => {
            prompt_loader::get_prompt("kb_search_execution_system")
        }
        AgentTemplates::KB_SEARCH_VALIDATION => {
            prompt_loader::get_prompt("kb_search_validation_system")
        }
        _ => None,
    };

    // If we have a system prompt, render it first then inject it into the context
    if let Some(prompt_template) = system_prompt {
        // Render the system prompt with the current context using the global HANDLEBARS
        let rendered_prompt = HANDLEBARS.render_template(prompt_template, &context_value)?;
        
        if let Some(obj) = context_value.as_object_mut() {
            obj.insert("system_prompt".to_string(), json!(rendered_prompt));
        }
    }

    HANDLEBARS.render(template_name, &context_value)
}
