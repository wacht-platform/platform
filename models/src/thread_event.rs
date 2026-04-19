use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::ToolApprovalDecision;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ThreadEvent {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub thread_id: i64,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub board_item_id: Option<i64>,
    pub event_type: String,
    pub status: String,
    pub priority: i32,
    pub payload: serde_json::Value,
    pub available_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub failed_at: Option<DateTime<Utc>>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub caused_by_run_id: Option<i64>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub caused_by_thread_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadConversationEventPayload {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub conversation_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponseReceivedEventPayload {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub conversation_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub request_message_id: i64,
    #[serde(default)]
    pub approvals: Vec<ToolApprovalDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRoutingEventPayload {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_item_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadAssignmentEventPayload {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub assignment_id: i64,
}

impl ThreadEvent {
    pub fn conversation_payload(&self) -> Option<ThreadConversationEventPayload> {
        match self.event_type.as_str() {
            event_type::USER_MESSAGE_RECEIVED | event_type::USER_INPUT_RECEIVED => {
                serde_json::from_value(self.payload.clone()).ok()
            }
            _ => None,
        }
    }

    pub fn assignment_execution_payload(&self) -> Option<ThreadAssignmentEventPayload> {
        (self.event_type == event_type::ASSIGNMENT_EXECUTION)
            .then(|| serde_json::from_value(self.payload.clone()).ok())
            .flatten()
    }

    pub fn assignment_outcome_review_payload(&self) -> Option<ThreadAssignmentEventPayload> {
        (self.event_type == event_type::ASSIGNMENT_OUTCOME_REVIEW)
            .then(|| serde_json::from_value(self.payload.clone()).ok())
            .flatten()
    }

    pub fn approval_response_received_payload(
        &self,
    ) -> Option<ApprovalResponseReceivedEventPayload> {
        (self.event_type == event_type::APPROVAL_RESPONSE_RECEIVED)
            .then(|| serde_json::from_value(self.payload.clone()).ok())
            .flatten()
    }

    pub fn task_routing_payload(&self) -> Option<TaskRoutingEventPayload> {
        (self.event_type == event_type::TASK_ROUTING)
            .then(|| serde_json::from_value(self.payload.clone()).ok())
            .flatten()
    }
}

pub mod status {
    pub const PENDING: &str = "pending";
    pub const CLAIMED: &str = "claimed";
    pub const COMPLETED: &str = "completed";
    pub const CANCELLED: &str = "cancelled";
    pub const FAILED: &str = "failed";
}

pub mod event_type {
    pub const USER_MESSAGE_RECEIVED: &str = "user_message_received";
    pub const USER_INPUT_RECEIVED: &str = "user_input_received";
    pub const APPROVAL_RESPONSE_RECEIVED: &str = "approval_response_received";
    pub const TASK_ROUTING: &str = "task_routing";
    pub const ASSIGNMENT_EXECUTION: &str = "assignment_execution";
    pub const ASSIGNMENT_OUTCOME_REVIEW: &str = "assignment_outcome_review";
    pub const CONTROL_STOP: &str = "thread_control_stop";
    pub const CONTROL_INTERRUPT: &str = "thread_control_interrupt";
}
