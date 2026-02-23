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
    m.insert(
        "context_research_repl_system",
        include_str!("prompts/context_research_repl_system.md"),
    );
    m.insert(
        "context_web_research_repl_system",
        include_str!("prompts/context_web_research_repl_system.md"),
    );

    // Memory prompts
    m.insert(
        "memory_consolidation_system",
        include_str!("prompts/memory_consolidation_prompt.md"),
    );

    m
});

pub fn get_prompt(key: &str) -> Option<&'static str> {
    PROMPTS.get(key).copied()
}
