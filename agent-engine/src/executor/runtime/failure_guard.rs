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
                    "`{label}` has failed {count} times in a row. Retrying only helps if you change the cause, not the arguments. Read the latest error and decide: is it something you can actually fix (wrong shape, missing precondition), or something you can't (a target that doesn't exist, no permission, an upstream that's down)? Fix it only if you can name the concrete change — otherwise stop and take a different route."
                ),
            );
            return Ok(());
        }

        self.store_transient_steer(
            "tool_failure_escalate_nudge",
            format!(
                "`{label}` has now failed {count} times with no progress — assume retrying it won't work. On your next turn, decide and act: if this is blocking and you can't fix it, escalate (hand it back / tell the coordinator what failed and what you'd need), or if it isn't essential, move ahead without it. Do not call `{label}` again with new inputs."
            ),
        );
        Ok(())
    }
}
