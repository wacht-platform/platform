use crate::llm::gemini::GEMINI_STRUCTURED_OUTPUT_TRUNCATED_MARKER;

use common::error::AppError;
use dto::json::agent_executor::{NextStep, NextStepDecision};
use models::{AiTool, AiToolType};
use std::collections::HashSet;

pub(crate) const DATABASE_ERROR_RETRY_STEP: &str = "database_error_retry";
pub(crate) const LLM_REQUEST_FAILED_STEP: &str = "llm_request_failed";
pub(crate) const STRUCTURED_OUTPUT_TRUNCATED_STEP: &str = "structured_output_truncated";
pub(crate) const TOOL_LOAD_REQUIRED_STEP: &str = "tool_load_required";
pub(crate) const RETRYABLE_EXECUTION_ERROR_STEP: &str = "retryable_execution_error";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCorrectionKind {
    DatabaseErrorRetry,
    LlmRequestFailed,
    StructuredOutputTruncated,
    ToolLoadRequired,
    RetryableExecutionError,
}

impl RuntimeCorrectionKind {
    pub(crate) fn step(self) -> &'static str {
        match self {
            Self::DatabaseErrorRetry => DATABASE_ERROR_RETRY_STEP,
            Self::LlmRequestFailed => LLM_REQUEST_FAILED_STEP,
            Self::StructuredOutputTruncated => STRUCTURED_OUTPUT_TRUNCATED_STEP,
            Self::ToolLoadRequired => TOOL_LOAD_REQUIRED_STEP,
            Self::RetryableExecutionError => RETRYABLE_EXECUTION_ERROR_STEP,
        }
    }

    pub(crate) fn reasoning(self) -> &'static str {
        match self {
            Self::DatabaseErrorRetry => {
                "Encountered a recoverable database error while progressing this thread. Retry the current step with the existing context."
            }
            Self::LlmRequestFailed => {
                "The previous model request failed before a valid response was produced. Retry the same step with the existing context and use the exact failure details from the prior correction record."
            }
            Self::StructuredOutputTruncated => {
                "Your previous structured response was truncated at the output limit. Keep reasoning and steer.message short. If you need to return long content, use startaction plus write_file, then steer with only a short summary or file reference. If the answer itself must be long, split it across multiple short steer messages instead of one oversized JSON string."
            }
            Self::ToolLoadRequired => {
                "Do not select startaction with unloaded external tools. Load the required external tools with loadtools first, then select startaction."
            }
            Self::RetryableExecutionError => {
                "The previous execution step failed before completion. Retry the same step if it is still valid, or choose a different valid next step using the exact failure details from the prior correction record."
            }
        }
    }

    pub(crate) fn confidence(self) -> f32 {
        match self {
            Self::RetryableExecutionError => 0.6,
            _ => 1.0,
        }
    }
}

pub(crate) struct NextStepDecisionValidationContext {
    pub long_think_mode_active: bool,
    pub long_think_credits_available: bool,
    pub long_think_input_tokens_available: u32,
    pub long_think_input_token_budget: u32,
    pub long_think_output_tokens_available: u32,
    pub long_think_output_token_budget: u32,
    pub can_abort_current_assignment_execution: bool,
    pub callable_tool_names: HashSet<String>,
    pub has_recent_startaction: bool,
}

pub(crate) struct RuntimeCorrectionContext<'a> {
    pub stage: &'a str,
    pub error_text: &'a str,
    pub loaded_external_tool_ids: &'a [i64],
    pub agent_tools: &'a [AiTool],
}

pub(crate) fn validate_next_step_decision(
    ctx: &NextStepDecisionValidationContext,
    decision: &NextStepDecision,
) -> Result<(), AppError> {
    if decision.reasoning.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Invalid next-step decision: reasoning must be non-empty".to_string(),
        ));
    }

    if !decision.confidence.is_finite() || !(0.0..=1.0).contains(&decision.confidence) {
        return Err(AppError::BadRequest(
            "Invalid next-step decision: confidence must be between 0.0 and 1.0".to_string(),
        ));
    }

    let has_steer = decision.steer.is_some();
    if ctx.long_think_mode_active && matches!(decision.next_step, NextStep::EnableLongThink) {
        return Err(AppError::BadRequest(
            "Invalid next-step decision: enablelongthink cannot be selected while long-think mode is already active".to_string(),
        ));
    }

    match decision.next_step {
        NextStep::Steer => {
            if !has_steer {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: steer requires steer payload".to_string(),
                ));
            }
            if let Some(steer) = decision.steer.as_ref() {
                if steer.message.trim().is_empty() {
                    return Err(AppError::BadRequest(
                        "Invalid next-step decision: steer message must be non-empty".to_string(),
                    ));
                }
                if steer.further_actions_required && steer.message.trim_end().ends_with('?') {
                    return Err(AppError::BadRequest(
                        "Invalid next-step decision: steer with further_actions_required=true cannot end with a question".to_string(),
                    ));
                }
                if let Some(attachments) = steer.attachments.as_ref() {
                    for attachment in attachments {
                        if attachment.path.trim().is_empty() {
                            return Err(AppError::BadRequest(
                                "Invalid next-step decision: steer attachment paths must be non-empty"
                                    .to_string(),
                            ));
                        }
                    }
                }
            }
        }
        NextStep::SearchTools => {
            let directive = decision.search_tools_directive.as_ref().ok_or_else(|| {
                AppError::BadRequest(
                    "Invalid next-step decision: searchtools requires search_tools_directive"
                        .to_string(),
                )
            })?;
            if directive
                .queries
                .iter()
                .all(|query| query.trim().is_empty())
            {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: searchtools requires at least one non-empty query"
                        .to_string(),
                ));
            }
        }
        NextStep::LoadTools => {
            let directive = decision.load_tools_directive.as_ref().ok_or_else(|| {
                AppError::BadRequest(
                    "Invalid next-step decision: loadtools requires load_tools_directive".to_string(),
                )
            })?;
            if directive
                .tool_names
                .iter()
                .all(|name| name.trim().is_empty())
            {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: loadtools requires at least one non-empty tool name"
                        .to_string(),
                ));
            }
        }
        NextStep::StartAction => {
            let directive = decision.startaction_directive.as_ref().ok_or_else(|| {
                AppError::BadRequest(
                    "Invalid next-step decision: startaction requires startaction_directive".to_string(),
                )
            })?;
            if directive.objective.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: startaction requires a non-empty objective".to_string(),
                ));
            }
            if directive
                .allowed_tools
                .iter()
                .all(|tool_name: &String| tool_name.trim().is_empty())
            {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: startaction requires at least one non-empty allowed_tools entry"
                        .to_string(),
                ));
            }
            let invalid_tools = directive
                .allowed_tools
                .iter()
                .map(|tool_name: &String| tool_name.trim())
                .filter(|tool_name: &&str| !tool_name.is_empty())
                .filter(|tool_name: &&str| !ctx.callable_tool_names.contains(*tool_name))
                .map(|tool_name: &str| tool_name.to_string())
                .collect::<Vec<_>>();
            if !invalid_tools.is_empty() {
                return Err(AppError::BadRequest(format!(
                    "Invalid next-step decision: startaction allowed_tools must be callable in the current mode. Invalid entries: {}",
                    invalid_tools.join(", ")
                )));
            }
        }
        NextStep::ContinueAction => {
            let directive = decision.continueaction_directive.as_ref().ok_or_else(|| {
                AppError::BadRequest(
                    "Invalid next-step decision: continueaction requires continueaction_directive"
                        .to_string(),
                )
            })?;
            if directive.guidance.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: continueaction requires non-empty guidance".to_string(),
                ));
            }
            if !ctx.has_recent_startaction {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: continueaction requires a recent startaction in conversation history"
                        .to_string(),
                ));
            }
        }
        NextStep::EnableLongThink => {
            if !ctx.long_think_credits_available {
                return Err(AppError::BadRequest(format!(
                    "Invalid next-step decision: enablelongthink requires remaining credits (input {} / {}, output {} / {})",
                    ctx.long_think_input_tokens_available,
                    ctx.long_think_input_token_budget,
                    ctx.long_think_output_tokens_available,
                    ctx.long_think_output_token_budget
                )));
            }
        }
        NextStep::Abort => {
            if !ctx.can_abort_current_assignment_execution {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: abort is only valid for assignment execution threads"
                        .to_string(),
                ));
            }
            let abort_directive = decision.abort_directive.as_ref().ok_or_else(|| {
                AppError::BadRequest(
                    "Invalid next-step decision: abort requires abort_directive".to_string(),
                )
            })?;
            if abort_directive.reason.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "Invalid next-step decision: abort requires a non-empty reason".to_string(),
                ));
            }
        }
    }

    Ok(())
}

pub(crate) fn classify_runtime_correction(
    ctx: &RuntimeCorrectionContext<'_>,
) -> Option<RuntimeCorrectionKind> {
    if is_structured_output_truncation_error(ctx.stage, ctx.error_text) {
        return Some(RuntimeCorrectionKind::StructuredOutputTruncated);
    }

    if requires_tool_load_correction(ctx) {
        return Some(RuntimeCorrectionKind::ToolLoadRequired);
    }

    None
}

fn is_structured_output_truncation_error(stage: &str, error_text: &str) -> bool {
    matches!(stage, "next-step-decision" | "decision-processing")
        && error_text.contains(GEMINI_STRUCTURED_OUTPUT_TRUNCATED_MARKER)
}

fn requires_tool_load_correction(ctx: &RuntimeCorrectionContext<'_>) -> bool {
    if ctx.stage != "next-step-decision" {
        return false;
    }

    let marker =
        "Invalid next-step decision: startaction allowed_tools must be callable in the current mode. Invalid entries:";
    let Some((_, invalid_segment)) = ctx.error_text.split_once(marker) else {
        return false;
    };
    let invalid_tools = invalid_segment
        .trim()
        .split(',')
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if invalid_tools.is_empty() {
        return false;
    }

    let unloaded_external_tools = ctx
        .agent_tools
        .iter()
        .filter(|tool| !matches!(tool.tool_type, AiToolType::Internal))
        .filter(|tool| !ctx.loaded_external_tool_ids.contains(&tool.id))
        .map(|tool| tool.name.as_str())
        .collect::<HashSet<_>>();

    invalid_tools
        .iter()
        .all(|tool_name| unloaded_external_tools.contains(*tool_name))
}
