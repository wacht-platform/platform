use crate::llm::NativeToolDefinition;
use serde_json::json;

pub fn note_tool() -> NativeToolDefinition {
    NativeToolDefinition {
        name: "note".to_string(),
        description: "Write a note to yourself. The note is recorded in the conversation history \
            so you can read it back on future turns. Use for: planning a multi-step sequence, \
            recording an observation from a prior tool result, reflecting on a mistake and how \
            to correct it, or anchoring a decision you need to remember. Notes do NOT execute \
            work and do NOT end the thread — they only leave a marker for your future self. \
            After a note, act on the next turn; do not take notes repeatedly without making progress."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "entry": {
                    "type": "string",
                    "description": "The note content. Be specific and grounded in what you just \
                        observed. Short is fine; substance matters more than length."
                }
            },
            "required": ["entry"]
        }),
    }
}

pub fn abort_tool() -> NativeToolDefinition {
    NativeToolDefinition {
        name: "abort_task".to_string(),
        description: "Abort the current assignment execution. \
            Use only when the task cannot proceed and must be stopped or escalated."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "reasoning": {
                    "type": "string",
                    "description": "Why the assignment cannot continue."
                },
                "outcome": {
                    "type": "string",
                    "enum": ["blocked", "return_to_coordinator"],
                    "description": "blocked: task is stuck and cannot proceed. \
                        return_to_coordinator: escalate back to the coordinator for re-routing."
                },
                "reason": {
                    "type": "string",
                    "description": "Concise explanation of why the assignment is being aborted, \
                        suitable for the task log."
                }
            },
            "required": ["reasoning", "outcome", "reason"]
        }),
    }
}
