use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use common::error::AppError;
use models::ConversationContent;

use crate::executor::core::AgentExecutor;
use crate::llm::{
    NativeToolDefinition, SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TerminalReviewDecision {
    pub decision: TerminalReviewChoice,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub artifacts: Option<Value>,
    #[serde(default)]
    pub blockers: Option<Value>,
    #[serde(default)]
    pub next_actions: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TerminalReviewChoice {
    Continue,
    Complete,
}

const CONTINUE_TOOL: &str = "mark_continue";
const COMPLETE_TOOL: &str = "mark_complete";

const REVIEW_SYSTEM_PROMPT_BASE: &str = r#"# terminal_review
# Spec for the pass that decides whether the agent should continue or complete
# after a text-only response.

[identity]
role = "honest reviewer"
inputs = ["recent execution history", "agent's most recent text-only response"]
output = "exactly one tool call: mark_continue OR mark_complete"

[mark_continue.triggers]
require_visible_history_justification = true
triggers_any = [
  "recoverable tool error (bad input, malformed parameter, transient retryable status) that the agent has not yet retried with corrected input — corrected input must be obvious from the error",
  "agent's own text says it is about to act THIS turn ('I'll save this to memory', 'let me now search...', 'next I'll build...', 'then I'll...') and the corresponding tool call is absent from this turn — the agent's stated intent to act now, not your inference, and not a handoff list of next steps for another lane",
  "LAST ITERATION TOOL ERRORS block present and errors reference paths/files/IDs/resources that other tool calls in this run created, modified, renamed, or deleted (agent lost track of its own state)",
  "LAST ITERATION TOOL ERRORS block present and the agent's terminal text claims completion or success without acknowledging the errors (errors arrived AFTER the agent composed text)",
]

[mark_complete.triggers]
default = true
triggers_any = [
  "final answers, clarifying questions, standby messages, acknowledgements, and status updates that report finished work or hand off to another lane — but NOT text that announces further actions the agent itself is about to take this run (that is mark_continue)",
  "tool errors that look like genuine API limitations, 404s on resources that may not exist, validation errors without an obvious correction (UNLESS they appear in a LAST ITERATION TOOL ERRORS block AND the agent's text did not acknowledge them)",
  "anything ambiguous — when uncertain, complete",
]

[hard_prohibitions]
list = [
  "never suggest hacks or workarounds — if the only path to retry is creative interpretation, complete",
  "never suggest revisiting explicitly-abandoned work",
  "never invent state or capabilities not visible in the history",
  "never recommend a tool by name in the hint",
  "never inflate continue cases to seem useful — default is complete",
]

[mark_continue.hint_shape]
length = "5-12 words"
form = "observation, never directive"
examples = [
  "promised memory save not emitted",
  "worklog 400 due to malformed entity_id",
  "tool error on bad input not retried",
]
fallback = "if you cannot phrase a concrete, honest observation grounded in the visible history, call mark_complete instead"

[mark_complete.summary]
required = true
length = "1-3 sentences"
must_describe = ["what was accomplished this run", "key decisions", "resulting state of the deliverable"]
audience = "next lane or reviewer reading cold"

[mark_complete.artifacts]
required = false
include_for = "files / paths produced or materially changed"
source_hints = ["visible tool_results for write_file, edit_file, append_file, gemini_generate_image, code-runner outputs and similar"]

[mark_complete.blockers]
required = false
include_for = "unresolved obstacles"

[mark_complete.next_actions]
required = false
include_for = "concrete follow-ups the next lane should consider"

[mark_complete.evidence_rule]
pull_from = "visible history"
forbidden = "invention"
pure_reply_exception = "for a pure conversation reply with no work product, summary may just describe what was said; artifacts/blockers/next_actions may be omitted"

[output_constraints]
emit = "exactly one tool call"
forbidden_text = "no free-form text"
function_name_format = "exactly as defined — do NOT prepend default_api. or any namespace prefix"
json_strings = "all string values in arguments must be properly JSON-escaped""#;

const ROLE_CONTEXT_CONVERSATION: &str = r#"

[role.conversation]
nature = "talking to a user"
terminal_text_is = "the user-facing reply — by design"
default = "complete"
complete_examples = [
  "reply to user",
  "clarifying question",
  "status update",
  "standby ('I'll keep checking')",
]
idling = "conversations idle and resume when the user replies"
continue_only_when = "agent's own text explicitly promised a tool call it then failed to emit ('I'll save this', 'creating the task now') AND the call is missing from recent tool history"
forbidden_continue_reasons = [
  "answer could be more thorough",
  "you'd have phrased it differently",
]"#;

const ROLE_CONTEXT_COORDINATOR: &str = r#"

[role.coordinator]
nature = "internal routing thread; idles between routing events"
terminal_text_is = "internal log entry, not user-facing"
default = "complete"
complete_when = "a routing decision was made and the board reflects it"
continue_only_when = "agent visibly attempted but did not complete a routing transition (created brief but didn't assign, said 'routing to X' but no assign_project_task call followed)"
forbidden_continue_reasons = [
  "brief could be more detailed",
  "a different routing would be 'better'",
]
mantra = "coordinators commit and move on"

[role.coordinator.summary_mandate]
authority = "mark_complete.summary is the only durable record of this routing turn that crosses thread boundaries"
audience = "upstream consumer's only context"
short_summary_on_substantive_turn = "defect — prefer mark_continue with hint 'coordinator summary missing required detail'"
substantive_turn = "created/updated assignments, changed board state, or made a routing decision"
trivial_acknowledgement_turn = "one-line summary acceptable""#;

const ROLE_CONTEXT_EXECUTOR: &str = r#"

[role.executor]
nature = "service thread working a single assignment"
terminal_text_is = "internal log"
default = "complete"
complete_when_any = [
  "slice deliverable was produced and journaled",
  "agent called abort_task (already escalating)",
]
continue_only_when = "agent's text said it would act THIS turn ('writing journal entry', 'saving the artifact', 'let me now search...', 'next I'll...') AND that tool call is missing from the turn"
forbidden_continue_reasons = [
  "journal could be richer",
  "slice could be more thorough",
]
division_of_labor = "reviewers catch quality; you catch missed mechanics""#;

const ROLE_CONTEXT_REVIEWER: &str = r#"

[role.reviewer]
nature = "judging executor work"
terminal_text_is = "accept / revise / reject decision"
default = "complete"
complete_when = "decision was stated and journaled"
continue_only_when = "agent claimed to verify a criterion but the corresponding verification tool call is missing from history"
forbidden_continue_reasons = ["second-guessing the verdict"]
disagreement_handling = "coordinator handles disagreement""#;

fn role_context(role: crate::executor::project::status_machine::ThreadRole) -> &'static str {
    use crate::executor::project::status_machine::ThreadRole;
    match role {
        ThreadRole::Conversation => ROLE_CONTEXT_CONVERSATION,
        ThreadRole::Coordinator => ROLE_CONTEXT_COORDINATOR,
        ThreadRole::Executor => ROLE_CONTEXT_EXECUTOR,
        ThreadRole::Reviewer => ROLE_CONTEXT_REVIEWER,
    }
}

const REVIEW_HISTORY_LIMIT: usize = 40;

impl AgentExecutor {
    pub(crate) async fn review_terminal_state(
        &self,
        prior_text: &str,
    ) -> Result<TerminalReviewDecision, AppError> {
        const MAX_REVIEW_ATTEMPTS: usize = 2;
        let history = self.build_terminal_review_messages(prior_text);
        let tools = review_tools();
        let system_prompt = format!(
            "{}{}",
            REVIEW_SYSTEM_PROMPT_BASE,
            role_context(self.current_thread_role())
        );

        let mut last_error: Option<AppError> = None;
        for attempt in 1..=MAX_REVIEW_ATTEMPTS {
            let config = SemanticLlmPromptConfig {
                response_json_schema: json!({}),
                temperature: Some(1.0),
                max_output_tokens: Some(200),
                reasoning_effort: None,
            };
            let mut request =
                SemanticLlmRequest::from_config(system_prompt.clone(), history.clone(), config);
            request.forced_tool_names =
                Some(vec![CONTINUE_TOOL.to_string(), COMPLETE_TOOL.to_string()]);
            match self
                .create_weak_llm()
                .await?
                .generate_tool_calls(request, tools.clone(), None)
                .await
            {
                Ok(output) => match decision_from_tool_calls(&output.calls) {
                    Some(decision) => return Ok(decision),
                    None => {
                        let error = AppError::Internal(format!(
                            "terminal review returned no recognized tool call (calls={:?}, text_present={})",
                            output
                                .calls
                                .iter()
                                .map(|c| c.tool_name.as_str())
                                .collect::<Vec<_>>(),
                            output.content_text.is_some()
                        ));
                        tracing::warn!(
                            thread_id = self.ctx.thread_id,
                            board_item_id = ?self.current_board_item_id(),
                            execution_run_id = self.ctx.execution_run_id,
                            attempt,
                            ?error,
                            "terminal review attempt produced no decision; will retry if attempts remain"
                        );
                        last_error = Some(error);
                    }
                },
                Err(error) => {
                    tracing::warn!(
                        thread_id = self.ctx.thread_id,
                        board_item_id = ?self.current_board_item_id(),
                        execution_run_id = self.ctx.execution_run_id,
                        attempt,
                        ?error,
                        "terminal review attempt failed; will retry if attempts remain"
                    );
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            AppError::Internal("terminal review exhausted retries with no error".to_string())
        }))
    }

    fn build_terminal_review_messages(&self, prior_text: &str) -> Vec<SemanticLlmMessage> {
        use crate::executor::context::conversation::format_history_timestamp;

        let mut entries: Vec<SemanticLlmMessage> = Vec::new();
        let conversations = self
            .conversations
            .iter()
            .rev()
            .take(REVIEW_HISTORY_LIMIT)
            .collect::<Vec<_>>();

        for conv in conversations.into_iter().rev() {
            let timestamp = match format_history_timestamp(&conv.created_at.to_rfc3339()) {
                Some(value) => value,
                None => continue,
            };
            match &conv.content {
                ConversationContent::UserMessage { message, .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        &format!("[{timestamp}] USER: {message}"),
                    ));
                }
                ConversationContent::Steer { message, .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        &format!("[{timestamp}] AGENT_TEXT: {message}"),
                    ));
                }
                ConversationContent::ToolResult {
                    tool_name,
                    status,
                    error,
                    ..
                } => {
                    let error_part = error
                        .as_ref()
                        .map(|e| format!(" error={e}"))
                        .unwrap_or_default();
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        &format!("[{timestamp}] TOOL: {tool_name} status={status}{error_part}"),
                    ));
                }
                ConversationContent::ClarificationRequest { .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        &format!("[{timestamp}] AGENT_ASKED_USER_QUESTION"),
                    ));
                }
                ConversationContent::ClarificationResponse { .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        &format!("[{timestamp}] USER_ANSWERED_QUESTION"),
                    ));
                }
                ConversationContent::ApprovalRequest { .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        &format!("[{timestamp}] AGENT_REQUESTED_APPROVAL"),
                    ));
                }
                ConversationContent::ApprovalResponse { .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        &format!("[{timestamp}] USER_RESPONDED_APPROVAL"),
                    ));
                }
                _ => {}
            }
        }

        entries.push(SemanticLlmMessage::text(
            "user",
            &format!("[just now] LATEST_AGENT_TEXT_ONLY: {prior_text}"),
        ));

        if let Some(rendered) = self.tool_error_window.last_render.as_ref() {
            let header = if rendered.kind == "brief" {
                "PRIOR-ITERATION TOOL ERRORS NOT ADDRESSED (agent was shown these earlier and did not act):\n"
            } else {
                "LAST ITERATION TOOL ERRORS (occurred AFTER the agent's latest text, so the agent has not yet seen them):\n"
            };
            let mut block = String::from(header);
            for err in &rendered.items {
                block.push_str(&format!(
                    "- [{}] {}\n  input: {}\n  error: {}\n",
                    err.timestamp, err.tool_name, err.input_preview, err.error
                ));
            }
            entries.push(SemanticLlmMessage::text("user", &block));
        }

        entries.push(SemanticLlmMessage::text(
            "user",
            "Call exactly one tool: mark_continue or mark_complete.",
        ));

        entries
    }
}

fn review_tools() -> Vec<NativeToolDefinition> {
    vec![
        NativeToolDefinition {
            name: CONTINUE_TOOL.to_string(),
            description:
                "Call this when the agent must continue because of a concrete, retryable failure that is clearly justified by the visible history."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hint": {
                        "type": "string",
                        "description": "5-12 word observation of what was left unaddressed. Observation form, never directive."
                    }
                },
                "required": ["hint"],
            }),
        },
        NativeToolDefinition {
            name: COMPLETE_TOOL.to_string(),
            description:
                "Call this when the agent's last response is acceptable as a terminal state. Default choice. Use whenever uncertain. Always provide a `summary`; include `artifacts`, `blockers`, and `next_actions` whenever the agent did concrete work the next lane will need to inherit."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "1-3 sentence handoff: what was accomplished in this run, key decisions, and the resulting state of the deliverable. Written for the next lane or reviewer to read cold. Required."
                    },
                    "artifacts": {
                        "type": "array",
                        "description": "Files, paths, or resources this run produced or materially changed. Strongly preferred when work touched disk.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string", "description": "Absolute path or stable identifier." },
                                "kind": { "type": "string", "description": "Short kind tag, e.g. 'file', 'image', 'video', 'report'." },
                                "note": { "type": "string", "description": "One-line description of what this artifact contains or represents." }
                            },
                            "required": ["path"]
                        }
                    },
                    "blockers": {
                        "type": "array",
                        "description": "Unresolved blockers, failed attempts, or constraints the next lane must know to make progress.",
                        "items": { "type": "string" }
                    },
                    "next_actions": {
                        "type": "array",
                        "description": "Concrete suggested follow-ups for the next assignment or reviewer.",
                        "items": { "type": "string" }
                    }
                },
                "required": ["summary"]
            }),
        },
    ]
}

fn decision_from_tool_calls(
    calls: &[crate::llm::GeneratedToolCall],
) -> Option<TerminalReviewDecision> {
    let call = calls.first()?;
    match call.tool_name.as_str() {
        CONTINUE_TOOL => {
            let hint = extract_hint(&call.arguments);
            Some(TerminalReviewDecision {
                decision: TerminalReviewChoice::Continue,
                hint,
                summary: None,
                artifacts: None,
                blockers: None,
                next_actions: None,
            })
        }
        COMPLETE_TOOL => {
            let args = &call.arguments;
            let summary = args
                .get("summary")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let artifacts = args.get("artifacts").cloned().filter(|v| !v.is_null());
            let blockers = args.get("blockers").cloned().filter(|v| !v.is_null());
            let next_actions = args.get("next_actions").cloned().filter(|v| !v.is_null());
            Some(TerminalReviewDecision {
                decision: TerminalReviewChoice::Complete,
                hint: None,
                summary,
                artifacts,
                blockers,
                next_actions,
            })
        }
        _ => None,
    }
}

fn extract_hint(args: &Value) -> Option<String> {
    args.get("hint")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
