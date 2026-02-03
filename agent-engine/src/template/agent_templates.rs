use handlebars::Handlebars;

pub fn register_all_templates(hb: &mut Handlebars) {
    hb.register_template_string(
        "step_decision_prompt",
        include_str!("templates/step_decision_prompt.hbs"),
    )
    .expect("Failed to register step_decision_prompt template");

    hb.register_template_string(
        "deep_reasoning_prompt",
        include_str!("templates/deep_reasoning_prompt.hbs"),
    )
    .expect("Failed to register deep_reasoning_prompt template");

    hb.register_template_string(
        "validation_prompt",
        include_str!("templates/validation_prompt.hbs"),
    )
    .expect("Failed to register validation_prompt template");

    hb.register_template_string(
        "summary_prompt",
        include_str!("templates/summary_prompt.hbs"),
    )
    .expect("Failed to register summary_prompt template");

    hb.register_template_string(
        "user_input_request_prompt",
        include_str!("templates/user_input_request_prompt.hbs"),
    )
    .expect("Failed to register user_input_request_prompt template");

    hb.register_template_string(
        "execution_summary_prompt",
        include_str!("templates/execution_summary_prompt.hbs"),
    )
    .expect("Failed to register execution_summary_prompt template");

    hb.register_template_string(
        "parameter_generation_prompt",
        include_str!("templates/parameter_generation_prompt.hbs"),
    )
    .expect("Failed to register parameter_generation_prompt template");

    hb.register_template_string(
        "context_search_derivation_prompt",
        include_str!("templates/context_search_derivation_prompt.hbs"),
    )
    .expect("Failed to register context_search_derivation_prompt template");

    hb.register_template_string(
        "memory_consolidation_prompt",
        include_str!("templates/memory_consolidation_prompt.hbs"),
    )
    .expect("Failed to register memory_consolidation_prompt template");
}
