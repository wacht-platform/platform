use handlebars::Handlebars;

pub fn register_all_templates(hb: &mut Handlebars) {
    // Templates for initial interaction
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

    hb.register_template_string(
        "tool_parameter_extraction_prompt",
        include_str!("templates/tool_parameter_extraction_prompt.hbs"),
    )
    .expect("Failed to register tool_parameter_extraction_prompt template");

    hb.register_template_string(
        "task_planning_prompt",
        include_str!("templates/task_planning_prompt.hbs"),
    )
    .expect("Failed to register task_planning_prompt template");

    // Templates for stage-based task execution
    hb.register_template_string(
        "task_exploration_prompt",
        include_str!("templates/task_exploration_prompt.hbs"),
    )
    .expect("Failed to register task_exploration_prompt template");

    hb.register_template_string(
        "task_action_prompt",
        include_str!("templates/task_action_prompt.hbs"),
    )
    .expect("Failed to register task_action_prompt template");

    hb.register_template_string(
        "task_correction_prompt",
        include_str!("templates/task_correction_prompt.hbs"),
    )
    .expect("Failed to register task_correction_prompt template");

    hb.register_template_string(
        "task_verification_prompt",
        include_str!("templates/task_verification_prompt.hbs"),
    )
    .expect("Failed to register task_verification_prompt template");

    // Memory evaluation template (still used)
    hb.register_template_string(
        "memory_evaluation_prompt",
        include_str!("templates/memory_evaluation_prompt.hbs"),
    )
    .expect("Failed to register memory_evaluation_prompt template");
}
