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

pub const LLM_CALL_LIMIT: u32 = 300;
pub const TOOL_CALL_LIMIT: u32 = 600;
pub const WALL_TIME_LIMIT_SECS: u64 = 45 * 60;

#[derive(Debug)]
pub struct BudgetCounter {
    pub llm_calls: u32,
    pub tool_calls: u32,
    pub tokens_used: u64,
    pub token_limit: Option<u64>,
    pub started_at: Instant,
}

impl Default for BudgetCounter {
    fn default() -> Self {
        Self {
            llm_calls: 0,
            tool_calls: 0,
            tokens_used: 0,
            token_limit: None,
            started_at: Instant::now(),
        }
    }
}

impl BudgetCounter {
    /// Seed a counter with a per-run token cap (None = uncapped).
    pub fn new(token_limit: Option<u64>) -> Self {
        Self {
            token_limit,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub enum BudgetExhausted {
    LlmCalls { used: u32, limit: u32 },
    ToolCalls { used: u32, limit: u32 },
    WallTime { used_secs: u64, limit_secs: u64 },
    Tokens { used: u64, limit: u64 },
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
            Self::Tokens { used, limit } => {
                format!("token budget exhausted: {used}/{limit} tokens used in this run")
            }
        }
    }
}

impl BudgetCounter {
    /// Returns `Err(...)` when any budget dimension is at or past its limit.
    /// Callers should preempt the run; the reason string lands in the abort
    /// directive so the coordinator sees what went wrong.
    pub fn check(&self) -> Result<(), BudgetExhausted> {
        if let Some(limit) = self.token_limit {
            if self.tokens_used >= limit {
                return Err(BudgetExhausted::Tokens {
                    used: self.tokens_used,
                    limit,
                });
            }
        }
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

    pub fn tick_tokens(&mut self, n: u64) {
        self.tokens_used = self.tokens_used.saturating_add(n);
    }
}
