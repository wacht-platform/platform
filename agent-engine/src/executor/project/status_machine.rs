use common::error::AppError;
use dto::json::agent_executor::UpdateProjectTaskParams;

const MIN_RESULT_SUMMARY_LEN: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadRole {
    Coordinator,
    Executor,
    Reviewer,
    Conversation,
}

impl ThreadRole {
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadRole::Coordinator => "coordinator",
            ThreadRole::Executor => "executor",
            ThreadRole::Reviewer => "reviewer",
            ThreadRole::Conversation => "conversation",
        }
    }
}

/// Statuses a given role is allowed to write via `update_project_task`.
///
/// Coordinator and conversation threads drive the full task lifecycle. Reviewers
/// must stay within reject/block/fail — anything else is structurally a coordinator
/// decision (accept = `completed`, dropped scope = `cancelled`, user input pending =
/// `needs_clarification`, waiting on children = `waiting_for_children`). Executors
/// don't typically call `update_project_task` to mark themselves done — they finish
/// via their assignment completion path — but they may need `blocked` or
/// `needs_clarification` mid-flight.
fn allowed_statuses_for_role(role: ThreadRole) -> &'static [&'static str] {
    match role {
        ThreadRole::Coordinator | ThreadRole::Conversation => &[
            "pending",
            "available",
            "claimed",
            "in_progress",
            "completed",
            "rejected",
            "blocked",
            "cancelled",
            "failed",
            "needs_clarification",
            "waiting_for_children",
        ],
        ThreadRole::Reviewer => &["rejected", "blocked", "failed"],
        ThreadRole::Executor => &["blocked", "needs_clarification"],
    }
}

/// Validate a proposed status write by `role`.
///
/// Returns a `BadRequest` whose body lists the allowed statuses for the role, so
/// the LLM that reads the tool error can self-correct on the next turn instead of
/// looping. The current status is informational — we don't gate on it yet, since
/// the existing flow doesn't fetch it before the write. Once we want full transition
/// gating (`completed → in_progress` reopen, etc.) we can extend this.
/// Statuses that demand a non-trivial `result_summary` (so the next reader —
/// coordinator, reviewer, or human — knows *why* the task landed here without
/// trawling the journal).
fn status_requires_summary(status: &str) -> bool {
    matches!(
        status,
        "completed" | "failed" | "blocked" | "rejected" | "needs_clarification"
    )
}

/// Statuses that demand at least one declared artifact path. Today only
/// `completed` qualifies — review verdicts and blocked states don't produce
/// deliverables under `/task/artifacts/`.
fn status_requires_artifacts(status: &str) -> bool {
    status == "completed"
}

/// Synchronous portion of terminal-payload validation: shape checks that don't
/// need to touch the sandbox. Run this first; if it passes, callers run
/// `validate_artifacts_exist` to confirm declared paths are real.
pub fn validate_terminal_payload_shape(
    next_status: &str,
    params: &UpdateProjectTaskParams,
) -> Result<(), AppError> {
    if status_requires_summary(next_status) {
        let summary = params.result_summary.as_deref().unwrap_or("").trim();
        if summary.len() < MIN_RESULT_SUMMARY_LEN {
            return Err(AppError::BadRequest(format!(
                "update_project_task: status `{next_status}` requires `result_summary` of at least \
                 {MIN_RESULT_SUMMARY_LEN} characters describing the outcome. Got {} chars.",
                summary.len()
            )));
        }
    }
    if status_requires_artifacts(next_status) {
        let artifacts = params.artifacts.as_deref().unwrap_or(&[]);
        if artifacts.is_empty() {
            return Err(AppError::BadRequest(format!(
                "update_project_task: status `{next_status}` requires at least one entry in \
                 `artifacts` (paths to deliverables produced, typically under `/task/artifacts/`)."
            )));
        }
        for path in artifacts {
            if path.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "update_project_task: `artifacts` entries must be non-empty paths.".to_string(),
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_status_for_role(role: ThreadRole, next: &str) -> Result<(), AppError> {
    let allowed = allowed_statuses_for_role(role);
    if allowed.contains(&next) {
        return Ok(());
    }
    Err(AppError::BadRequest(format!(
        "update_project_task: status `{next}` is not allowed for role `{}`. Allowed here: {}.",
        role.as_str(),
        allowed.join(", ")
    )))
}
