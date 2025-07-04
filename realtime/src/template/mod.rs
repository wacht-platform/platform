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
    pub const TASK_ANALYSIS: &'static str = "task_analysis_prompt";
    pub const ACKNOWLEDGMENT: &'static str = "acknowledgment_prompt";
    pub const TOOL_ANALYSIS: &'static str = "tool_analysis_prompt";
}

pub fn render_template(
    template_name: &str,
    context: &impl serde::Serialize,
) -> Result<String, handlebars::RenderError> {
    HANDLEBARS.render(template_name, context)
}
