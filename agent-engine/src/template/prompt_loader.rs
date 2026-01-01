use std::collections::HashMap;
use std::sync::LazyLock;

// Static loading of all prompts at compile time
static PROMPTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // Core decision prompts
    m.insert(
        "step_decision_system",
        include_str!("prompts/step_decision_system.md"),
    );
    m.insert(
        "deep_reasoning_system",
        include_str!("prompts/deep_reasoning_system.md"),
    );
    m.insert(
        "validation_system",
        include_str!("prompts/validation_prompt.md"),
    );
    m.insert("summary_system", include_str!("prompts/summary_prompt.md"));
    m.insert(
        "user_input_request_system",
        include_str!("prompts/user_input_request_system.md"),
    );
    m.insert(
        "execution_summary_system",
        include_str!("prompts/execution_summary_system.md"),
    );

    // Tool & context prompts
    m.insert(
        "parameter_generation_system",
        include_str!("prompts/parameter_generation_prompt.md"),
    );
    m.insert(
        "context_search_derivation_system",
        include_str!("prompts/context_search_derivation_prompt.md"),
    );

    // Workflow node prompts
    m.insert(
        "switch_case_evaluation_system",
        include_str!("prompts/switch_case_evaluation_system.md"),
    );
    m.insert(
        "trigger_evaluation_system",
        include_str!("prompts/trigger_evaluation_system.md"),
    );

    m
});

pub fn get_prompt(key: &str) -> Option<&'static str> {
    PROMPTS.get(key).copied()
}
