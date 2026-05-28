mod ask_user;
mod notify_user;
mod resolve_user_feedback;

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

pub fn ask_user_tool() -> NativeToolDefinition {
    NativeToolDefinition {
        name: "ask_user".to_string(),
        description: "Only channel for asking the user anything (clarification, choice, confirmation, missing fact). Never end a turn with a question in plain text — use this tool. Last resort: prefer resolving via other tools, context, or a sensible default. Ends the turn; pauses until answered; one pending set at a time. Each question: `id`, `text`, `answer_kind`. \
            \n\nPick `answer_kind.kind` by the SHAPE of the answer, not by how many words it is:\
            \n- single_choice — exactly one selection from a known finite set (choices REQUIRED, allow_other? when the set might be incomplete). DEFAULT for 'which/pick/select one' phrasing. Use even for 2-option questions that aren't literal yes/no (e.g. 'staging or production?').\
            \n- multi_choice — zero-or-more selections (choices REQUIRED, min_selected? max_selected?). Use ONLY when 'select all that apply' is the natural framing: feature toggles, integrations to enable, tags, days of week. Never set `max_selected: 1` — that's single_choice. If at least one must be picked, set `min_selected: 1` rather than reframing.\
            \n- yes_no — literal yes/no. Prefer over single_choice for binary boolean questions.\
            \n- confirm — irreversible/destructive action gate ('Delete', 'Send'); REQUIRES confirm_label + cancel_label. Not for general questions.\
            \n- free_text — open-ended; use when the option space is unknown, unbounded, or freeform (a name, a description, a URL). If you can enumerate 3-7 plausible options, prefer single_choice with allow_other.\
            \n- number (min?, max?, unit?) — numeric input.\
            \n- date (yyyy-mm-dd, min_date?, max_date?).\
            \n\nFor choice kinds: order options by likelihood (most common first), keep `label` short, use `description` only for one-line disambiguation. Optional top-level `context` explains why you're asking."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "description": "One or more questions to present together. IDs must be unique within the set.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Stable id; the answer payload references it."
                            },
                            "text": {
                                "type": "string",
                                "description": "Question text shown to the user."
                            },
                            "answer_kind": {
                                "type": "object",
                                "description": "Tagged-union answer shape. Set `kind` and the fields appropriate for that kind; omit fields that don't apply.",
                                "properties": {
                                    "kind": {
                                        "type": "string",
                                        "enum": ["free_text", "single_choice", "multi_choice", "yes_no", "number", "date", "confirm"],
                                        "description": "Discriminator selecting the shape."
                                    },
                                    "placeholder": {
                                        "type": "string",
                                        "description": "free_text: optional placeholder shown in the input."
                                    },
                                    "max_length": {
                                        "type": "integer",
                                        "minimum": 1,
                                        "description": "free_text: optional character cap."
                                    },
                                    "choices": {
                                        "type": "array",
                                        "description": "single_choice / multi_choice: REQUIRED list of options. Ordered by likelihood (most common first). Each option has a stable `value`, a display `label`, and optional `description`. If exactly one answer makes sense, use single_choice; if multiple toggles can be picked together, use multi_choice.",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "value": {
                                                    "type": "string",
                                                    "description": "Stable id for this option (returned in the answer)."
                                                },
                                                "label": {
                                                    "type": "string",
                                                    "description": "Human-readable label shown to the user."
                                                },
                                                "description": {
                                                    "type": "string",
                                                    "description": "Optional one-line clarification of this option."
                                                }
                                            },
                                            "required": ["value", "label"]
                                        }
                                    },
                                    "allow_other": {
                                        "type": "boolean",
                                        "description": "single_choice: when true, a free-text 'other' value is accepted in addition to the listed choices."
                                    },
                                    "min_selected": {
                                        "type": "integer",
                                        "minimum": 0,
                                        "description": "multi_choice: minimum number of selections (default 0)."
                                    },
                                    "max_selected": {
                                        "type": "integer",
                                        "minimum": 2,
                                        "description": "multi_choice: maximum number of selections. Must be >= 2 — if only one selection is allowed, use single_choice instead."
                                    },
                                    "min": {
                                        "type": "number",
                                        "description": "number: lower bound (inclusive)."
                                    },
                                    "max": {
                                        "type": "number",
                                        "description": "number: upper bound (inclusive)."
                                    },
                                    "unit": {
                                        "type": "string",
                                        "description": "number: optional display unit (e.g. 'minutes')."
                                    },
                                    "min_date": {
                                        "type": "string",
                                        "description": "date: ISO yyyy-mm-dd lower bound (inclusive)."
                                    },
                                    "max_date": {
                                        "type": "string",
                                        "description": "date: ISO yyyy-mm-dd upper bound (inclusive)."
                                    },
                                    "confirm_label": {
                                        "type": "string",
                                        "description": "confirm: REQUIRED label for the confirm button (e.g. 'Approve')."
                                    },
                                    "cancel_label": {
                                        "type": "string",
                                        "description": "confirm: REQUIRED label for the cancel button (e.g. 'Reject')."
                                    }
                                },
                                "required": ["kind"]
                            }
                        },
                        "required": ["id", "text", "answer_kind"]
                    }
                },
                "context": {
                    "type": "string",
                    "description": "Optional one-paragraph explanation of why you're asking, shown to the user above the questions."
                }
            },
            "required": ["questions"]
        }),
    }
}

pub fn notify_user_tool() -> NativeToolDefinition {
    NativeToolDefinition {
        name: "notify_user".to_string(),
        description: "Push a short progress notice and end the turn (thread goes idle until next user input). For in-progress updates that aren't the final answer and don't need a reply. Not `ask_user`, not a final text reply. Conversation threads only."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The notice to display. One or two sentences; concrete and grounded in what just happened. Don't ask a question here."
                }
            },
            "required": ["message"]
        }),
    }
}

pub fn resolve_user_feedback_tool() -> NativeToolDefinition {
    NativeToolDefinition {
        name: "resolve_user_feedback".to_string(),
        description: "Mark user-feedback comment(s) on the current task as resolved. Call after acting on the feedback (or deciding no action). Summary is a one-line plain-English description of what was done (or why not). Resolved comments stay tagged `[resolved]` in the timeline. Coordinator/executor threads with an active board_item only.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "comment_ids": {
                    "type": "array",
                    "minItems": 1,
                    "description": "Stable string ids of the comments being resolved (from the `id=` field shown in the timeline).",
                    "items": { "type": "string" }
                },
                "resolution": {
                    "type": "string",
                    "description": "One-line summary of what you did about the feedback. Be specific."
                }
            },
            "required": ["comment_ids", "resolution"]
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
                "outcome": {
                    "type": "string",
                    "enum": ["blocked", "return_to_coordinator"],
                    "description": "blocked: task is stuck and cannot proceed. \
                        return_to_coordinator: escalate back to the coordinator for re-routing."
                },
                "reason": {
                    "type": "string",
                    "description": "Explanation of why the assignment cannot continue. \
                        Concrete enough for the task log and the coordinator to act on."
                }
            },
            "required": ["outcome", "reason"]
        }),
    }
}
