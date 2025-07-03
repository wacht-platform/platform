use handlebars::Handlebars;

pub fn register_all_templates(hb: &mut Handlebars) {
    hb.register_template_string(
        "agent_system_prompt",
        include_str!("templates/agent_system_prompt.hbs"),
    )
    .expect("Failed to register agent_system_prompt template");

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
        "validation_prompt",
        include_str!("templates/validation_prompt.hbs"),
    )
    .expect("Failed to register validation_prompt template");

    hb.register_template_string(
        "condition_evaluation_prompt",
        include_str!("templates/condition_evaluation_prompt.hbs"),
    )
    .expect("Failed to register condition_evaluation_prompt template");

    hb.register_template_string(
        "context_generation_prompt",
        include_str!("templates/context_generation_prompt.hbs"),
    )
    .expect("Failed to register context_generation_prompt template");

    hb.register_template_string(
        "tool_analysis_prompt",
        include_str!("templates/tool_analysis_prompt.hbs"),
    )
    .expect("Failed to register tool_analysis_prompt template");

    hb.register_template_string(
        "capabilities_list",
        include_str!("templates/capabilities_list.hbs"),
    )
    .expect("Failed to register capabilities_list template");
}
