use std::collections::HashMap;
use std::sync::LazyLock;

static PROMPTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    m.insert(
        "conversation_agent_system",
        concat!(
            include_str!("prompts/shared_operating_style.md"),
            "\n\n",
            include_str!("prompts/memory_discipline.md"),
            "\n\n",
            include_str!("prompts/conversation_agent_system.md"),
        ),
    );
    m.insert(
        "coordinator_system",
        concat!(
            include_str!("prompts/shared_operating_style.md"),
            "\n\n",
            include_str!("prompts/coordinator_system.md"),
        ),
    );
    m.insert(
        "service_execution_system",
        concat!(
            include_str!("prompts/shared_operating_style.md"),
            "\n\n",
            include_str!("prompts/memory_discipline.md"),
            "\n\n",
            include_str!("prompts/service_execution_system.md"),
        ),
    );
    m.insert(
        "reviewer_system",
        concat!(
            include_str!("prompts/shared_operating_style.md"),
            "\n\n",
            include_str!("prompts/memory_discipline.md"),
            "\n\n",
            include_str!("prompts/reviewer_system.md"),
        ),
    );
    m.insert(
        "execution_summary_system",
        include_str!("prompts/execution_summary_system.md"),
    );
    m
});

pub fn get_prompt(key: &str) -> Option<&'static str> {
    PROMPTS.get(key).copied()
}
