use serde::{Deserialize, Serialize};
use serde_json::json;

use common::error::AppError;
use models::ConversationContent;

use crate::executor::core::AgentExecutor;
use crate::llm::{SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TerminalReviewDecision {
    pub decision: TerminalReviewChoice,
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TerminalReviewChoice {
    Continue,
    Complete,
}

const REVIEW_SYSTEM_PROMPT: &str = "\
You are an honest reviewer. Read the recent execution history and the agent's most recent text-only response. Decide whether to `continue` or `complete`. Output one structured decision.

Return `continue` ONLY when there is a concrete, retryable failure where the *justification for retrying is clear from the visible history*. Specifically:
- A tool returned a recoverable error (bad input, malformed parameter, transient retryable status) that the agent has not yet retried with corrected input — and the corrected input is obvious from the error.
- The agent emitted text that explicitly promised a tool call (\"I'll save this to memory\", \"I'll log the worklog\") and did not make the call. The call must be the agent's own stated intent, not your inference.

Otherwise return `complete`. This includes:
- Final answers, clarifying questions, status updates, standby messages, acknowledgements.
- Tool errors that look like genuine API limitations, 404s on resources that may not exist, validation errors without an obvious correction.
- Anything ambiguous. When uncertain, `complete`.

Hard prohibitions:
- Never suggest hacks or workarounds. If the only path to retry is creative interpretation, return `complete`.
- Never suggest revisiting explicitly-abandoned work.
- Never invent state or capabilities not visible in the history.
- Never recommend a tool by name in the hint.
- Never inflate continue cases to seem useful. Default is `complete`.

If `continue`, `hint` is a 5-12 word observation describing *what was left unaddressed*, in observation form: \"promised memory save not emitted\", \"worklog 400 due to malformed entity_id\", \"tool error on bad input not retried\". Never directive. If you cannot phrase a concrete, honest observation grounded in the visible history, return `complete` instead.";

const REVIEW_HISTORY_LIMIT: usize = 40;

impl AgentExecutor {
    pub(crate) async fn review_terminal_state(
        &self,
        prior_text: &str,
    ) -> Result<TerminalReviewDecision, AppError> {
        const MAX_REVIEW_ATTEMPTS: usize = 2;
        let history = self.build_terminal_review_messages(prior_text);
        let schema = json!({
            "type": "object",
            "properties": {
                "decision": {
                    "type": "string",
                    "enum": ["continue", "complete"],
                },
                "hint": {
                    "type": "string",
                    "description": "5-12 word observation when decision is continue. Omit when complete."
                }
            },
            "required": ["decision"],
        });

        let mut last_error: Option<AppError> = None;
        for attempt in 1..=MAX_REVIEW_ATTEMPTS {
            let config = SemanticLlmPromptConfig {
                response_json_schema: schema.clone(),
                temperature: Some(0.1),
                max_output_tokens: Some(200),
                reasoning_effort: None,
            };
            let request = SemanticLlmRequest::from_config(
                REVIEW_SYSTEM_PROMPT.to_string(),
                history.clone(),
                config,
            );
            match self
                .create_weak_llm()
                .await?
                .generate_structured_from_prompt::<TerminalReviewDecision>(request, None)
                .await
            {
                Ok(response) => return Ok(response.value),
                Err(error) => {
                    tracing::warn!(
                        thread_id = self.ctx.thread_id,
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
        let mut entries: Vec<SemanticLlmMessage> = Vec::new();
        let conversations = self
            .conversations
            .iter()
            .rev()
            .take(REVIEW_HISTORY_LIMIT)
            .collect::<Vec<_>>();

        for conv in conversations.into_iter().rev() {
            match &conv.content {
                ConversationContent::UserMessage { message, .. } => {
                    entries.push(SemanticLlmMessage::text("user", &format!("USER: {message}")));
                }
                ConversationContent::Steer { message, .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        &format!("AGENT_TEXT: {message}"),
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
                        &format!("TOOL: {tool_name} status={status}{error_part}"),
                    ));
                }
                ConversationContent::ClarificationRequest { .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        "AGENT_ASKED_USER_QUESTION",
                    ));
                }
                ConversationContent::ClarificationResponse { .. } => {
                    entries.push(SemanticLlmMessage::text("user", "USER_ANSWERED_QUESTION"));
                }
                ConversationContent::ApprovalRequest { .. } => {
                    entries.push(SemanticLlmMessage::text(
                        "user",
                        "AGENT_REQUESTED_APPROVAL",
                    ));
                }
                ConversationContent::ApprovalResponse { .. } => {
                    entries.push(SemanticLlmMessage::text("user", "USER_RESPONDED_APPROVAL"));
                }
                _ => {}
            }
        }

        entries.push(SemanticLlmMessage::text(
            "user",
            &format!("LATEST_AGENT_TEXT_ONLY: {prior_text}"),
        ));
        entries.push(SemanticLlmMessage::text(
            "user",
            "Decide: continue or complete.",
        ));

        entries
    }
}
