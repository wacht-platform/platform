use handlebars::Handlebars;

pub fn register_all_templates(hb: &mut Handlebars) {
    hb.register_template_string(
        "agent_loop_live_context",
        include_str!("templates/agent_loop_live_context.hbs"),
    )
    .expect("Failed to register agent_loop_live_context template");

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
        "task_workspace_brief",
        include_str!("templates/task_workspace_brief.hbs"),
    )
    .expect("Failed to register task_workspace_brief template");

    hb.register_template_string(
        "project_instructions",
        include_str!("templates/project_instructions.hbs"),
    )
    .expect("Failed to register project_instructions template");
}
