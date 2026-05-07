use models::{AiAgentWithFeatures, ApprovalAction};
use regex::Regex;

use crate::tools::external::VIRTUAL_TOOL_NAME_PREFIX;

const MCP_TOOL_NAME_PREFIX: &str = "mcp_";

pub fn resolve_approval_action(agent: &AiAgentWithFeatures, tool_name: &str) -> ApprovalAction {
    for rule in &agent.tool_approval_rules {
        match Regex::new(&rule.pattern) {
            Ok(re) => {
                if re.is_match(tool_name) {
                    return rule.action;
                }
            }
            Err(err) => {
                tracing::warn!(
                    agent_id = agent.id,
                    pattern = %rule.pattern,
                    error = %err,
                    "approval rule regex failed to compile; skipping"
                );
            }
        }
    }

    if tool_name.starts_with(MCP_TOOL_NAME_PREFIX) {
        return if agent.require_approval_mcp {
            ApprovalAction::Review
        } else {
            ApprovalAction::Allow
        };
    }
    if tool_name.starts_with(VIRTUAL_TOOL_NAME_PREFIX) {
        return if agent.require_approval_virtual {
            ApprovalAction::Review
        } else {
            ApprovalAction::Allow
        };
    }

    if let Some(tool) = agent.tools.iter().find(|t| t.name == tool_name) {
        return tool.approval_action;
    }

    ApprovalAction::Allow
}
