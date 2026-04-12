use std::collections::HashMap;
use std::sync::LazyLock;

// Static loading of all prompts at compile time
static PROMPTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // Core decision prompts
    m.insert(
        "next_step_decision_system",
        include_str!("prompts/next_step_decision_system.md"),
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
