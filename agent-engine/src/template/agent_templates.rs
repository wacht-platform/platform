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

    hb.register_template_string(
        "workflow_validation_prompt",
        include_str!("templates/workflow_validation_prompt.hbs"),
    )
    .expect("Failed to register workflow_validation_prompt template");

    hb.register_template_string(
        "summary_prompt",
        include_str!("templates/summary_prompt.hbs"),
    )
    .expect("Failed to register summary_prompt template");

    hb.register_template_string(
        "step_decision_prompt",
        include_str!("templates/step_decision_prompt.hbs"),
    )
    .expect("Failed to register step_decision_prompt template");

    hb.register_template_string(
        "context_search_derivation_prompt",
        include_str!("templates/context_search_derivation_prompt.hbs"),
    )
    .expect("Failed to register context_search_derivation_prompt template");

    hb.register_template_string(
        "knowledge_base_search_prompt",
        include_str!("templates/knowledge_base_search_prompt.hbs"),
    )
    .expect("Failed to register knowledge_base_search_prompt template");

    hb.register_template_string(
        "kb_search_execution_prompt",
        include_str!("templates/kb_search_execution_prompt.hbs"),
    )
    .expect("Failed to register kb_search_execution_prompt template");

    hb.register_template_string(
        "kb_search_validation_prompt",
        include_str!("templates/kb_search_validation_prompt.hbs"),
    )
    .expect("Failed to register kb_search_validation_prompt template");

    hb.register_template_string(
        "memory_evaluation_prompt",
        include_str!("templates/memory_evaluation_prompt.hbs"),
    )
    .expect("Failed to register memory_evaluation_prompt template");

    hb.register_template_string(
        "switch_case_evaluation_prompt",
        include_str!("templates/switch_case_evaluation_prompt.hbs"),
    )
    .expect("Failed to register switch_case_evaluation_prompt template");

    hb.register_template_string(
        "trigger_evaluation_prompt",
        include_str!("templates/trigger_evaluation_prompt.hbs"),
    )
    .expect("Failed to register trigger_evaluation_prompt template");

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
}
