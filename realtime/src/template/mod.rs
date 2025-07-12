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
    pub const IDEATION: &'static str = "ideation_prompt";
    pub const CONTEXT_GATHERING: &'static str = "context_gathering_prompt";
    pub const TASK_BREAKDOWN: &'static str = "task_breakdown_prompt";
    pub const TASK_EXECUTION: &'static str = "task_execution_prompt";
    pub const VALIDATION: &'static str = "validation_prompt";
    pub const PARAMETER_GENERATION: &'static str = "parameter_generation_prompt";
    pub const WORKFLOW_VALIDATION: &'static str = "workflow_validation_prompt";
}

pub fn render_template(
    template_name: &str,
    context: &impl serde::Serialize,
) -> Result<String, handlebars::RenderError> {
    HANDLEBARS.render(template_name, context)
}
