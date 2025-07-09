use handlebars::Handlebars;

pub fn register_all_templates(hb: &mut Handlebars) {
    hb.register_template_string(
        "acknowledgment_prompt",
        include_str!("templates/acknowledgment_prompt.hbs"),
    )
    .expect("Failed to register acknowledgment_prompt template");

    hb.register_template_string(
        "ideation_prompt",
        include_str!("templates/ideation_prompt.hbs"),
    )
    .expect("Failed to register ideation_prompt template");

    hb.register_template_string(
        "context_gathering_prompt",
        include_str!("templates/context_gathering_prompt.hbs"),
    )
    .expect("Failed to register context_gathering_prompt template");

    hb.register_template_string(
        "task_breakdown_prompt",
        include_str!("templates/task_breakdown_prompt.hbs"),
    )
    .expect("Failed to register task_breakdown_prompt template");

    hb.register_template_string(
        "task_execution_prompt",
        include_str!("templates/task_execution_prompt.hbs"),
    )
    .expect("Failed to register task_execution_prompt template");

    hb.register_template_string(
        "validation_prompt",
        include_str!("templates/validation_prompt.hbs"),
    )
    .expect("Failed to register validation_prompt template");

    hb.register_template_string(
        "parameter_generation_prompt",
        include_str!("templates/parameter_generation_prompt.hbs"),
    )
    .expect("Failed to register parameter_generation_prompt template");
}
