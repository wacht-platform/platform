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
    pub const AGENT_LOOP_LIVE_CONTEXT: &'static str = "agent_loop_live_context";

    // Tool & context templates
    pub const WORKER_TASK_ROUTING_CONTEXT: &'static str = "worker_task_routing_context";
    pub const WORKER_ASSIGNMENT_EXECUTION_CONTEXT: &'static str =
        "worker_assignment_execution_context";
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

/// Renders the project-scoped instructions block written into
/// `agent_threads.system_instructions` at thread creation and injected into the
/// live context at runtime. Same content for every thread kind — role-specific
/// behavior lives in the role system prompts.
pub fn render_project_instructions(
    project_name: &str,
    project_brief: Option<&str>,
    custom_rules: Option<&str>,
) -> Result<String, AppError> {
    let context = serde_json::json!({
        "project_name": project_name,
        "project_brief": project_brief,
        "custom_rules": custom_rules,
    });

    HANDLEBARS
        .render("project_instructions", &context)
        .map(|rendered| rendered.trim().to_string())
        .map_err(|e| {
            AppError::Internal(format!("Failed to render project_instructions: {e}"))
        })
}
