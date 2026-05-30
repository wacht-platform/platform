use super::core::AgentExecutor;
use common::error::AppError;
use std::collections::BTreeSet;

const NUDGE_AT: usize = 3;
const ESCALATE_NUDGE_AT: usize = 5;
const FLAIL_ESCALATE_AT: usize = 5;

impl AgentExecutor {
    pub(crate) fn update_consecutive_tool_failures(
        &mut self,
        failed: &BTreeSet<String>,
        succeeded: &BTreeSet<String>,
    ) {
        // Cross-tool streak: any batch that still carries a failure keeps it alive, even when the
        // model switches tools each turn (which resets the same-tool counter below). This catches a
        // model thrashing across many different tools, not just hammering one.
        if failed.is_empty() {
            self.consecutive_failed_batches = 0;
        } else {
            self.consecutive_failed_batches = self.consecutive_failed_batches.saturating_add(1);
        }

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

    // The exact error from the latest failing batch (tracked tool if present), so the nudge can
    // hand the real reason to the model instead of generic advice. No classification — just render.
    fn last_failure_detail(&self, label: &str) -> String {
        let found = self
            .tool_error_window
            .errors
            .iter()
            .rev()
            .find(|e| e.tool_name == label)
            .or_else(|| self.tool_error_window.errors.last());
        match found {
            Some(e) => format!("\nLast error: {}", e.error.chars().take(300).collect::<String>()),
            None => String::new(),
        }
    }

    pub(crate) async fn apply_tool_failure_guard(&mut self) -> Result<(), AppError> {
        let count = self.consecutive_tool_failure_count;
        let flail = self.consecutive_failed_batches;
        let label = self
            .last_failed_tool_label
            .clone()
            .unwrap_or_else(|| "that tool".to_string());
        let detail = self.last_failure_detail(&label);

        if count >= ESCALATE_NUDGE_AT {
            self.store_transient_steer(
                "tool_failure_escalate_nudge",
                format!(
                    "`{label}` has failed {count} times — treat it as not workable right now and stop retrying it.{detail}\n\
                     Do (pick exactly one, next turn): continue the task without this tool, or report what failed and what you'd need to unblock it. Leaving it unfinished is acceptable.\n\
                     Don't: call `{label}` again; retry it with tweaked inputs; re-issue a call that just failed; pad the turn by restating the error or apologizing."
                ),
            );
            return Ok(());
        }
        if count >= NUDGE_AT {
            self.store_transient_steer(
                "tool_failure_retry_nudge",
                format!(
                    "`{label}` has failed {count} times — don't force it.{detail}\n\
                     Decide from that error: retry only if it names one concrete thing YOU can change (a field, a precondition, an id) and you can make that exact change. If it doesn't, stop calling it and either work around it or report it blocked — that's a fine outcome. If you were trying to discover or load a tool, call `load_tools` with the exact name instead of searching again.\n\
                     Don't: re-send the same or a near-identical `{label}` call hoping for a different result; invent new values just to clear the error."
                ),
            );
            return Ok(());
        }

        // The same-tool counter never climbed, but failures keep landing across different tools.
        if flail >= FLAIL_ESCALATE_AT {
            self.store_transient_steer(
                "tool_failure_flailing_nudge",
                format!(
                    "The last {flail} tool batches have each ended in a failure, across different tools — you're thrashing, not progressing.{detail}\n\
                     Do (next turn): stop trying new tool/argument variations to clear these errors. Either finish the task with what already works, or report it blocked with what failed and what you'd need. If you were hunting for a tool, `load_tools` it by exact name instead of searching.\n\
                     Don't: keep rotating through tools or arguments hoping one slips through."
                ),
            );
        }
        Ok(())
    }
}
