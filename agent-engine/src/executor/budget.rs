//! Per-execution-run resource budgets.
//!
//! Caps how much an executor (or reviewer) can burn in a single run before the
//! runtime preempts it and hands control back to the coordinator. The coordinator
//! sees the abort reason and decides whether to extend, retry with a tighter
//! brief, or mark the task `failed`.
//!
//! Tracked per-run (in-memory) rather than per-task (DB-backed). A preempted
//! task that gets reassigned starts with a fresh budget — that's intentional for
//! now: the failure mode we're catching is a single run going off the rails, not
//! a death-spiral across many runs. Promote to persistent if death-spirals show
//! up in production.
//!
//! Compaction cycles are deliberately not capped — the user wants compaction to
//! run as needed.

use std::time::Instant;

pub const LLM_CALL_LIMIT: u32 = 150;
pub const TOOL_CALL_LIMIT: u32 = 600;
pub const WALL_TIME_LIMIT_SECS: u64 = 45 * 60;

#[derive(Debug)]
pub struct BudgetCounter {
    pub llm_calls: u32,
    pub tool_calls: u32,
    pub started_at: Instant,
}

impl Default for BudgetCounter {
    fn default() -> Self {
        Self {
            llm_calls: 0,
            tool_calls: 0,
            started_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum BudgetExhausted {
    LlmCalls { used: u32, limit: u32 },
    ToolCalls { used: u32, limit: u32 },
    WallTime { used_secs: u64, limit_secs: u64 },
}

impl BudgetExhausted {
    pub fn reason(&self) -> String {
        match self {
            Self::LlmCalls { used, limit } => {
                format!("LLM call budget exhausted: {used}/{limit} calls used in this run")
            }
            Self::ToolCalls { used, limit } => {
                format!("tool call budget exhausted: {used}/{limit} calls used in this run")
            }
            Self::WallTime {
                used_secs,
                limit_secs,
            } => format!(
                "wall time budget exhausted: {used_secs}s elapsed, {limit_secs}s allowed per run"
            ),
        }
    }
}

impl BudgetCounter {
    /// Returns `Err(...)` when any budget dimension is at or past its limit.
    /// Callers should preempt the run; the reason string lands in the abort
    /// directive so the coordinator sees what went wrong.
    pub fn check(&self) -> Result<(), BudgetExhausted> {
        if self.llm_calls >= LLM_CALL_LIMIT {
            return Err(BudgetExhausted::LlmCalls {
                used: self.llm_calls,
                limit: LLM_CALL_LIMIT,
            });
        }
        if self.tool_calls >= TOOL_CALL_LIMIT {
            return Err(BudgetExhausted::ToolCalls {
                used: self.tool_calls,
                limit: TOOL_CALL_LIMIT,
            });
        }
        let elapsed = self.started_at.elapsed().as_secs();
        if elapsed >= WALL_TIME_LIMIT_SECS {
            return Err(BudgetExhausted::WallTime {
                used_secs: elapsed,
                limit_secs: WALL_TIME_LIMIT_SECS,
            });
        }
        Ok(())
    }

    pub fn tick_llm(&mut self) {
        self.llm_calls = self.llm_calls.saturating_add(1);
    }

    pub fn tick_tools(&mut self, n: usize) {
        self.tool_calls = self.tool_calls.saturating_add(n as u32);
    }
}
