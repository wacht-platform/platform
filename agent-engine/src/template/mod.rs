use chrono::Utc;
use common::error::AppError;
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
    pub const STEP_DECISION: &'static str = "next_step_decision_prompt";
    pub const STEP_DECISION_LIVE_CONTEXT: &'static str = "next_step_decision_live_context";
    pub const EXECUTION_SUMMARY: &'static str = "execution_summary_prompt";

    // Tool & context templates
    pub const WORKER_TASK_ROUTING_CONTEXT: &'static str = "worker_task_routing_context";
    pub const WORKER_ASSIGNMENT_EXECUTION_CONTEXT: &'static str =
        "worker_assignment_execution_context";
    pub const WORKER_ASSIGNMENT_OUTCOME_REVIEW_CONTEXT: &'static str =
        "worker_assignment_outcome_review_context";
    pub const TASK_WORKSPACE_BRIEF: &'static str = "task_workspace_brief";
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
        AgentTemplates::STEP_DECISION => prompt_loader::get_prompt("next_step_decision_system"),
        AgentTemplates::EXECUTION_SUMMARY => prompt_loader::get_prompt("execution_summary_system"),
        _ => None,
    };

    // If we have a system prompt, render it first then inject it into the context
    if let Some(prompt_template) = system_prompt {
        // Render the system prompt with the current context using the global HANDLEBARS
        let rendered_prompt = HANDLEBARS
            .render_template(prompt_template, &context)
            .unwrap();

        if let Some(obj) = context.as_object_mut() {
            obj.insert("system_prompt".to_string(), Value::String(rendered_prompt));
        }
    }

    HANDLEBARS.render(template_name, &context)
}

pub fn render_template_only(
    template_name: &str,
    context: &Value,
) -> Result<String, handlebars::RenderError> {
    HANDLEBARS.render(template_name, context)
}

pub fn render_prompt_text(prompt_name: &str, context: &Value) -> Result<String, AppError> {
    let prompt_template = prompt_loader::get_prompt(prompt_name)
        .ok_or_else(|| AppError::Internal(format!("Unknown prompt template: {prompt_name}")))?;
    HANDLEBARS
        .render_template(prompt_template, context)
        .map_err(|e| AppError::Internal(format!("Failed to render prompt {prompt_name}: {e}")))
}

pub fn render_template_json_with_prompt<T>(
    template_name: &str,
    context: Value,
) -> Result<T, AppError>
where
    T: serde::de::DeserializeOwned,
{
    let rendered = render_template_with_prompt(template_name, context)
        .map_err(|e| AppError::Internal(format!("Failed to render template {template_name}: {e}")))?;
    serde_json::from_str(&rendered).map_err(|e| {
        AppError::Internal(format!(
            "Failed to parse rendered template {template_name} as JSON: {e}"
        ))
    })
}

pub fn render_template_json<T>(template_name: &str, context: &Value) -> Result<T, AppError>
where
    T: serde::de::DeserializeOwned,
{
    let rendered = HANDLEBARS
        .render(template_name, context)
        .map_err(|e| AppError::Internal(format!("Failed to render template {template_name}: {e}")))?;
    serde_json::from_str(&rendered).map_err(|e| {
        AppError::Internal(format!(
            "Failed to parse rendered template {template_name} as JSON: {e}"
        ))
    })
}
