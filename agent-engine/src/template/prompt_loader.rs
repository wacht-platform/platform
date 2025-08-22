use std::collections::HashMap;
use std::sync::LazyLock;

// Static loading of all prompts at compile time
static PROMPTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // System prompts
    m.insert(
        "acknowledgment_system",
        include_str!("prompts/acknowledgment_system.md"),
    );
    m.insert(
        "context_gathering_system",
        include_str!("prompts/context_gathering_system.md"),
    );
    m.insert(
        "context_search_derivation_system",
        include_str!("prompts/context_search_derivation_prompt.md"),
    );
    m.insert(
        "ideation_system",
        include_str!("prompts/ideation_prompt.md"),
    );
    m.insert(
        "parameter_generation_system",
        include_str!("prompts/parameter_generation_prompt.md"),
    );
    m.insert(
        "step_decision_system",
        include_str!("prompts/step_decision_system.md"),
    );
    m.insert("summary_system", include_str!("prompts/summary_prompt.md"));
    m.insert(
        "task_breakdown_system",
        include_str!("prompts/task_breakdown_system.md"),
    );
    m.insert(
        "task_execution_system",
        include_str!("prompts/task_execution_prompt.md"),
    );
    m.insert(
        "validation_system",
        include_str!("prompts/validation_prompt.md"),
    );
    m.insert(
        "workflow_validation_system",
        include_str!("prompts/workflow_validation_prompt.md"),
    );
    m.insert(
        "knowledge_base_search_system",
        include_str!("prompts/knowledge_base_search_system.md"),
    );
    m.insert(
        "memory_evaluation_system",
        include_str!("prompts/memory_evaluation_system.md"),
    );

    m
});

pub fn get_prompt(key: &str) -> Option<&'static str> {
    PROMPTS.get(key).copied()
}
