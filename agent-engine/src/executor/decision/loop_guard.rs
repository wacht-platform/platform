use super::core::AgentExecutor;
use dto::json::agent_executor::{AbortOutcome, NextStep, NextStepDecision};
use std::collections::HashSet;

impl AgentExecutor {
    pub(crate) fn active_task_graph_has_unfinished_nodes(&self) -> bool {
        let Some(snapshot) = self.task_graph_snapshot.as_ref() else {
            return false;
        };

        let graph_status = snapshot
            .get("graph")
            .and_then(|graph| graph.get("status"))
            .and_then(|status| status.as_str())
            .unwrap_or_default();

        if graph_status != models::thread_task_graph::status::GRAPH_ACTIVE {
            return false;
        }

        snapshot
            .get("nodes")
            .and_then(|nodes| nodes.as_array())
            .map(|nodes| {
                nodes.iter().any(|node| {
                    matches!(
                        node.get("status").and_then(|status| status.as_str()),
                        Some(
                            models::thread_task_graph::status::NODE_PENDING
                                | models::thread_task_graph::status::NODE_IN_PROGRESS
                                | models::thread_task_graph::status::NODE_FAILED
                        )
                    )
                })
            })
            .unwrap_or(false)
    }

    pub(super) fn track_decision_pattern(&mut self, decision: &NextStepDecision) -> usize {
        let signature = Self::decision_loop_signature(decision);
        let repeated = self
            .last_decision_signature
            .as_deref()
            .map(|previous| Self::decision_signatures_similar(previous, &signature))
            .unwrap_or(false);

        if !repeated {
            self.repeated_decision_count = 0;
            self.last_decision_signature = Some(signature);
            return self.repeated_decision_count;
        }

        self.repeated_decision_count += 1;
        self.last_decision_signature = Some(signature);
        self.repeated_decision_count
    }

    fn decision_loop_signature(decision: &NextStepDecision) -> String {
        match decision.next_step {
            NextStep::Steer => {
                let msg = decision
                    .steer
                    .as_ref()
                    .map(|a| Self::normalize_loop_text(&a.message))
                    .unwrap_or_default();
                let further_actions_required = decision
                    .steer
                    .as_ref()
                    .map(|a| a.further_actions_required)
                    .unwrap_or(false);
                format!("steer:{further_actions_required}:{msg}")
            }
            NextStep::SearchTools => format!(
                "searchtools:{}",
                decision
                    .search_tools_directive
                    .as_ref()
                    .map(|directive| {
                        directive
                            .queries
                            .iter()
                            .map(|query| Self::normalize_loop_text(query))
                            .collect::<Vec<_>>()
                            .join("|")
                    })
                    .unwrap_or_else(|| "missing".to_string())
            ),
            NextStep::LoadTools => format!(
                "loadtools:{}",
                decision
                    .load_tools_directive
                    .as_ref()
                    .map(|directive| directive.tool_names.join("|"))
                    .unwrap_or_else(|| "missing".to_string())
            ),
            NextStep::StartAction => decision
                .startaction_directive
                .as_ref()
                .map(|directive| {
                    format!(
                        "startaction:{}:{}",
                        Self::normalize_loop_text(&directive.objective),
                        directive
                            .allowed_tools
                            .iter()
                            .map(|tool| Self::normalize_loop_text(tool))
                            .collect::<Vec<_>>()
                            .join("|")
                    )
                })
                .unwrap_or_else(|| "startaction:missing".to_string()),
            NextStep::ContinueAction => decision
                .continueaction_directive
                .as_ref()
                .map(|directive| {
                    format!(
                        "continueaction:{}",
                        Self::normalize_loop_text(&directive.guidance)
                    )
                })
                .unwrap_or_else(|| "continueaction:missing".to_string()),
            NextStep::EnableLongThink => "enablelongthink".to_string(),
            NextStep::Abort => format!(
                "abort:{}:{}",
                decision
                    .abort_directive
                    .as_ref()
                    .map(|d| match d.outcome {
                        AbortOutcome::Blocked => "blocked",
                        AbortOutcome::ReturnToCoordinator => "return_to_coordinator",
                    })
                    .unwrap_or("missing"),
                decision
                    .abort_directive
                    .as_ref()
                    .map(|d| Self::normalize_loop_text(&d.reason))
                    .unwrap_or_default()
            ),
        }
    }

    fn normalize_loop_text(input: &str) -> String {
        input
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn decision_signatures_similar(previous: &str, current: &str) -> bool {
        if previous == current {
            return true;
        }

        let (previous_kind, previous_payload) = Self::split_decision_signature(previous);
        let (current_kind, current_payload) = Self::split_decision_signature(current);
        if previous_kind != current_kind {
            return false;
        }

        if previous_payload.is_empty() || current_payload.is_empty() {
            return previous_payload == current_payload;
        }

        Self::word_similarity(previous_payload, current_payload) >= 0.35
    }

    fn split_decision_signature(signature: &str) -> (&str, &str) {
        match signature.split_once(':') {
            Some((kind, payload)) => (kind, payload),
            None => (signature, ""),
        }
    }

    fn word_similarity(left: &str, right: &str) -> f32 {
        let left_tokens = Self::tokenize_similarity_text(left);
        let right_tokens = Self::tokenize_similarity_text(right);

        if left_tokens.is_empty() || right_tokens.is_empty() {
            return 0.0;
        }

        let intersection = left_tokens.intersection(&right_tokens).count() as f32;
        let union = left_tokens.union(&right_tokens).count() as f32;
        if union == 0.0 {
            return 0.0;
        }
        intersection / union
    }

    fn tokenize_similarity_text(input: &str) -> HashSet<String> {
        input
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter_map(|token| {
                let token = token.trim().to_ascii_lowercase();
                if token.len() >= 2 {
                    Some(token)
                } else {
                    None
                }
            })
            .collect()
    }
}
