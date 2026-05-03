use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub payload: serde_json::Value,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub caused_by_thread_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRoutingEventPayload {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_item_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_priority: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_fields: Vec<TaskRoutingFieldChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assignment_result_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRoutingFieldChange {
    pub field: String,
    pub from: String,
    pub to: String,
}

pub mod routing_reason {
    pub const TASK_CREATED: &str = "task_created";
    pub const TASK_UPDATED: &str = "task_updated";
    pub const ASSIGNMENT_PREEMPTED: &str = "assignment_preempted";
    pub const ASSIGNMENT_COMPLETED: &str = "assignment_completed";
    pub const TASK_CANCELLED: &str = "task_cancelled";
    pub const USER_RESPONDED: &str = "user_responded";
    pub const USER_FEEDBACK: &str = "user_feedback";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadAssignmentEventPayload {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub assignment_id: i64,
}

impl ThreadEvent {
    pub fn assignment_execution_payload(&self) -> Option<ThreadAssignmentEventPayload> {
        (self.event_type == event_type::ASSIGNMENT_EXECUTION)
            .then(|| serde_json::from_value(self.payload.clone()).ok())
            .flatten()
    }

    pub fn task_routing_payload(&self) -> Option<TaskRoutingEventPayload> {
        (self.event_type == event_type::TASK_ROUTING)
            .then(|| serde_json::from_value(self.payload.clone()).ok())
            .flatten()
    }
}

pub mod event_type {
    pub const USER_MESSAGE_RECEIVED: &str = "user_message_received";
    pub const APPROVAL_RESPONSE_RECEIVED: &str = "approval_response_received";
    pub const TASK_ROUTING: &str = "task_routing";
    pub const ASSIGNMENT_EXECUTION: &str = "assignment_execution";
}
