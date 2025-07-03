use handlebars::Handlebars;
use std::sync::LazyLock;

pub mod agent_templates;
pub mod helpers;

pub use agent_templates::*;
pub use helpers::*;

pub static HANDLEBARS: LazyLock<Handlebars<'static>> = LazyLock::new(|| {
    let mut hb = Handlebars::new();

    helpers::register_all_helpers(&mut hb);
    agent_templates::register_all_templates(&mut hb);

    hb
});

pub struct AgentTemplates;

impl AgentTemplates {
    pub const SYSTEM_PROMPT: &'static str = "agent_system_prompt";
    pub const TASK_ANALYSIS: &'static str = "task_analysis_prompt";
    pub const ACKNOWLEDGMENT: &'static str = "acknowledgment_prompt";
    pub const VALIDATION: &'static str = "validation_prompt";
    pub const CONDITION_EVALUATION: &'static str = "condition_evaluation_prompt";
    pub const CONTEXT_GENERATION: &'static str = "context_generation_prompt";
    pub const TOOL_ANALYSIS: &'static str = "tool_analysis_prompt";
    pub const CAPABILITIES_LIST: &'static str = "capabilities_list";
}

pub fn render_template(
    template_name: &str,
    context: &impl serde::Serialize,
) -> Result<String, handlebars::RenderError> {
    HANDLEBARS.render(template_name, context)
}

pub fn render_template_string(
    template: &str,
    context: &impl serde::Serialize,
) -> Result<String, handlebars::RenderError> {
    HANDLEBARS.render_template(template, context)
}

pub fn get_template_names() -> Vec<String> {
    HANDLEBARS.get_templates().keys().cloned().collect()
}

pub fn template_exists(name: &str) -> bool {
    HANDLEBARS.get_template(name).is_some()
}
