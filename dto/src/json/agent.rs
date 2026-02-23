use models::ConversationRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;

use crate::json::{PlatformEventPayload, PlatformFunctionPayload};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStreamMessageType {
    ConversationMessage,
    PlatformEvent,
    PlatformFunction,
    UserInputRequest,
    ChildAgentCompleted,
}

impl AgentStreamMessageType {
    pub fn as_header_value(self) -> &'static str {
        match self {
            Self::ConversationMessage => "conversation_message",
            Self::PlatformEvent => "platform_event",
            Self::PlatformFunction => "platform_function",
            Self::UserInputRequest => "user_input_request",
            Self::ChildAgentCompleted => "child_agent_completed",
        }
    }

    pub fn webhook_event_name(self) -> &'static str {
        match self {
            Self::ConversationMessage => "execution_context.message",
            Self::PlatformEvent => "execution_context.platform_event",
            Self::PlatformFunction => "execution_context.platform_function",
            Self::UserInputRequest => "execution_context.user_input_request",
            Self::ChildAgentCompleted => "execution_context.child_agent_completed",
        }
    }
}

impl FromStr for AgentStreamMessageType {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "conversation_message" => Ok(Self::ConversationMessage),
            "platform_event" => Ok(Self::PlatformEvent),
            "platform_function" => Ok(Self::PlatformFunction),
            "user_input_request" => Ok(Self::UserInputRequest),
            "child_agent_completed" => Ok(Self::ChildAgentCompleted),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamEvent {
    PlatformEvent(String, serde_json::Value),
    PlatformFunction(String, serde_json::Value),
    ConversationMessage(ConversationRecord),
    UserInputRequest(models::ConversationContent),
    ChildAgentCompleted {
        child_context_id: i64,
        status: String,
        summary: Option<Value>,
    },
}

impl StreamEvent {
    pub fn message_type(&self) -> AgentStreamMessageType {
        match self {
            StreamEvent::PlatformEvent(_, _) => AgentStreamMessageType::PlatformEvent,
            StreamEvent::PlatformFunction(_, _) => AgentStreamMessageType::PlatformFunction,
            StreamEvent::ConversationMessage(_) => AgentStreamMessageType::ConversationMessage,
            StreamEvent::UserInputRequest(_) => AgentStreamMessageType::UserInputRequest,
            StreamEvent::ChildAgentCompleted { .. } => AgentStreamMessageType::ChildAgentCompleted,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChildAgentCompletedPayload {
    pub child_context_id: i64,
    pub status: String,
    pub summary: Option<Value>,
}

pub fn encode_stream_event(
    event: &StreamEvent,
) -> Result<(AgentStreamMessageType, Vec<u8>), serde_json::Error> {
    match event {
        StreamEvent::ConversationMessage(conversation_content) => {
            let payload = serde_json::to_vec(conversation_content)?;
            Ok((AgentStreamMessageType::ConversationMessage, payload))
        }
        StreamEvent::PlatformEvent(event_label, event_data) => {
            let payload = serde_json::to_vec(&PlatformEventPayload {
                event_label: event_label.clone(),
                event_data: event_data.clone(),
            })?;
            Ok((AgentStreamMessageType::PlatformEvent, payload))
        }
        StreamEvent::PlatformFunction(function_name, function_data) => {
            let payload = serde_json::to_vec(&PlatformFunctionPayload {
                function_name: function_name.clone(),
                function_data: function_data.clone(),
            })?;
            Ok((AgentStreamMessageType::PlatformFunction, payload))
        }
        StreamEvent::UserInputRequest(user_input_content) => {
            let payload = serde_json::to_vec(user_input_content)?;
            Ok((AgentStreamMessageType::UserInputRequest, payload))
        }
        StreamEvent::ChildAgentCompleted {
            child_context_id,
            status,
            summary,
        } => {
            let payload = serde_json::to_vec(&ChildAgentCompletedPayload {
                child_context_id: *child_context_id,
                status: status.clone(),
                summary: summary.clone(),
            })?;
            Ok((AgentStreamMessageType::ChildAgentCompleted, payload))
        }
    }
}

pub fn decode_stream_event(
    message_type: AgentStreamMessageType,
    payload: &[u8],
) -> Result<StreamEvent, serde_json::Error> {
    match message_type {
        AgentStreamMessageType::ConversationMessage => {
            Ok(StreamEvent::ConversationMessage(serde_json::from_slice::<
                ConversationRecord,
            >(payload)?))
        }
        AgentStreamMessageType::PlatformEvent => {
            let event = serde_json::from_slice::<PlatformEventPayload>(payload)?;
            Ok(StreamEvent::PlatformEvent(
                event.event_label,
                event.event_data,
            ))
        }
        AgentStreamMessageType::PlatformFunction => {
            let event = serde_json::from_slice::<PlatformFunctionPayload>(payload)?;
            Ok(StreamEvent::PlatformFunction(
                event.function_name,
                event.function_data,
            ))
        }
        AgentStreamMessageType::UserInputRequest => {
            Ok(StreamEvent::UserInputRequest(serde_json::from_slice::<
                models::ConversationContent,
            >(payload)?))
        }
        AgentStreamMessageType::ChildAgentCompleted => {
            let event = serde_json::from_slice::<ChildAgentCompletedPayload>(payload)?;
            Ok(StreamEvent::ChildAgentCompleted {
                child_context_id: event.child_context_id,
                status: event.status,
                summary: event.summary,
            })
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_name: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename = "self_evaluation")]
pub struct SelfEvaluation {
    pub progress_assessment: ProgressAssessment,
    pub quality_assessment: QualityAssessment,
    pub approach_evaluation: ApproachEvaluation,
    pub next_steps: NextSteps,
    #[serde(rename = "lessons_learned", default)]
    pub lessons_learned: Vec<LessonLearned>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LessonLearned {
    #[serde(rename = "insight")]
    pub insight: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressAssessment {
    pub percentage_complete: u8,
    pub on_track: bool,
    pub reasoning: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QualityAssessment {
    pub quality_score: u8,
    pub meets_requirements: bool,
    #[serde(rename = "issues_found", default)]
    pub issues_found: Vec<Issue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    #[serde(rename = "issue")]
    pub issue: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApproachEvaluation {
    pub current_approach_effective: bool,
    #[serde(default)]
    pub suggested_adjustments: SuggestedAdjustments,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SuggestedAdjustments {
    #[serde(rename = "adjustment", default)]
    pub adjustments: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Adjustment {
    #[serde(rename = "adjustment")]
    pub adjustment: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NextSteps {
    pub recommendation: EvaluationRecommendation,
    pub reasoning: String,
    pub proposed_actions: Option<Vec<ProposedAction>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvaluationRecommendation {
    Continue,
    Adjust,
    Retry,
    Complete,
    Abort,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposedAction {
    #[serde(rename = "type")]
    pub action_type: String,
    pub description: String,
}
