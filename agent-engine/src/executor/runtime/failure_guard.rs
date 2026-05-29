use super::core::AgentExecutor;
use common::error::AppError;
use std::collections::BTreeSet;

const NUDGE_AT: usize = 3;
const ESCALATE_NUDGE_AT: usize = 5;

impl AgentExecutor {
    pub(crate) fn update_consecutive_tool_failures(
        &mut self,
        failed: &BTreeSet<String>,
        succeeded: &BTreeSet<String>,
    ) {
        if let Some(tracked) = self.last_failed_tool_label.clone() {
            if succeeded.contains(&tracked) {
                self.last_failed_tool_label = None;
                self.consecutive_tool_failure_count = 0;
                return;
            }
            if failed.contains(&tracked) {
                self.consecutive_tool_failure_count =
                    self.consecutive_tool_failure_count.saturating_add(1);
                return;
            }
        }

        match failed.iter().next() {
            Some(first_failed) => {
                self.last_failed_tool_label = Some(first_failed.clone());
                self.consecutive_tool_failure_count = 1;
            }
            None => {
                self.last_failed_tool_label = None;
                self.consecutive_tool_failure_count = 0;
            }
        }
    }

    pub(crate) async fn apply_tool_failure_guard(&mut self) -> Result<(), AppError> {
        let count = self.consecutive_tool_failure_count;
        if count < NUDGE_AT {
            return Ok(());
        }
        let label = self
            .last_failed_tool_label
            .clone()
            .unwrap_or_else(|| "that tool".to_string());

        if count < ESCALATE_NUDGE_AT {
            self.store_transient_steer(
                "tool_failure_retry_nudge",
                format!(
                    "`{label}` has failed {count} times — don't force it.\n\
                     Do: retry only if the error names one concrete thing to change (a field, a precondition, the id) and you can make that exact change. If you can't, stop calling it and either work around it or report it as blocked — that's a fine outcome, not a shortfall.\n\
                     Don't: re-send the same or a near-identical `{label}` call hoping for a different result; invent new values just to clear the error; repeat a call that already failed the same way."
                ),
            );
            return Ok(());
        }

        self.store_transient_steer(
            "tool_failure_escalate_nudge",
            format!(
                "`{label}` has failed {count} times — treat it as not workable right now and stop retrying it.\n\
                 Do (pick exactly one, next turn): continue the task without this tool, or report what failed and what you'd need to unblock it. Leaving it unfinished is acceptable.\n\
                 Don't: call `{label}` again; retry it with tweaked inputs; re-issue a call that just failed; pad the turn by restating the error or apologizing."
            ),
        );
        Ok(())
    }
}
