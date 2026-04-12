use handlebars::Handlebars;

pub fn register_all_templates(hb: &mut Handlebars) {
    hb.register_template_string(
        "next_step_decision_prompt",
        include_str!("templates/next_step_decision_prompt.hbs"),
    )
    .expect("Failed to register next_step_decision_prompt template");

    hb.register_template_string(
        "next_step_decision_live_context",
        include_str!("templates/next_step_decision_live_context.hbs"),
    )
    .expect("Failed to register next_step_decision_live_context template");

    hb.register_template_string(
        "execution_summary_prompt",
        include_str!("templates/execution_summary_prompt.hbs"),
    )
    .expect("Failed to register execution_summary_prompt template");

    hb.register_template_string(
        "worker_task_routing_context",
        include_str!("templates/worker_task_routing_context.hbs"),
    )
    .expect("Failed to register worker_task_routing_context template");

    hb.register_template_string(
        "worker_assignment_execution_context",
        include_str!("templates/worker_assignment_execution_context.hbs"),
    )
    .expect("Failed to register worker_assignment_execution_context template");

    hb.register_template_string(
        "worker_assignment_outcome_review_context",
        include_str!("templates/worker_assignment_outcome_review_context.hbs"),
    )
    .expect("Failed to register worker_assignment_outcome_review_context template");

    hb.register_template_string(
        "task_workspace_brief",
        include_str!("templates/task_workspace_brief.hbs"),
    )
    .expect("Failed to register task_workspace_brief template");
}
