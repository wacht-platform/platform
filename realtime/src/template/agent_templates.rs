use handlebars::Handlebars;

pub fn register_all_templates(hb: &mut Handlebars) {
    hb.register_template_string(
        "task_analysis_prompt",
        include_str!("templates/task_analysis_prompt.hbs"),
    )
    .expect("Failed to register task_analysis_prompt template");

    hb.register_template_string(
        "acknowledgment_prompt",
        include_str!("templates/acknowledgment_prompt.hbs"),
    )
    .expect("Failed to register acknowledgment_prompt template");

    hb.register_template_string(
        "tool_analysis_prompt",
        include_str!("templates/tool_analysis_prompt.hbs"),
    )
    .expect("Failed to register tool_analysis_prompt template");
}
