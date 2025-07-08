use handlebars::Handlebars;
use std::sync::LazyLock;

pub mod agent_templates;
pub mod helpers;

pub static HANDLEBARS: LazyLock<Handlebars<'static>> = LazyLock::new(|| {
    let mut hb = Handlebars::new();

    helpers::register_all_helpers(&mut hb);
    agent_templates::register_all_templates(&mut hb);

    hb
});

pub struct AgentTemplates;

impl AgentTemplates {
    pub const ACKNOWLEDGMENT: &'static str = "acknowledgment_prompt";
    pub const TOOL_PARAMETER_EXTRACTION: &'static str = "tool_parameter_extraction_prompt";
    pub const TASK_PLANNING: &'static str = "task_planning_prompt";
    pub const TASK_EXPLORATION: &'static str = "task_exploration_prompt";
    pub const TASK_ACTION: &'static str = "task_action_prompt";
    pub const TASK_CORRECTION: &'static str = "task_correction_prompt";
    pub const TASK_VERIFICATION: &'static str = "task_verification_prompt";
    pub const MEMORY_EVALUATION: &'static str = "memory_evaluation_prompt";
}

pub fn render_template(
    template_name: &str,
    context: &impl serde::Serialize,
) -> Result<String, handlebars::RenderError> {
    HANDLEBARS.render(template_name, context)
}
