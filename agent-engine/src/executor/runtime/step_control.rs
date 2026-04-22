pub(crate) const DATABASE_ERROR_RETRY_STEP: &str = "database_error_retry";
pub(crate) const LLM_REQUEST_FAILED_STEP: &str = "llm_request_failed";
pub(crate) const RETRYABLE_EXECUTION_ERROR_STEP: &str = "retryable_execution_error";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCorrectionKind {
    DatabaseErrorRetry,
    LlmRequestFailed,
    RetryableExecutionError,
}

impl RuntimeCorrectionKind {
    pub(crate) fn step(self) -> &'static str {
        match self {
            Self::DatabaseErrorRetry => DATABASE_ERROR_RETRY_STEP,
            Self::LlmRequestFailed => LLM_REQUEST_FAILED_STEP,
            Self::RetryableExecutionError => RETRYABLE_EXECUTION_ERROR_STEP,
        }
    }

    pub(crate) fn reasoning(self) -> &'static str {
        match self {
            Self::DatabaseErrorRetry => {
                "Encountered a recoverable database error while progressing this thread. Retry the current step with the existing context."
            }
            Self::LlmRequestFailed => {
                "The previous model request failed before a valid response was produced. Retry the same turn with the existing context."
            }
            Self::RetryableExecutionError => {
                "The previous turn failed before completion. Retry the same move if it is still valid, or pick a different one using the failure details from the prior correction record."
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
