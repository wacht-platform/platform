use models::ConversationRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;

use crate::json::PlatformEventPayload;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStreamMessageType {
    ConversationMessage,
    PlatformEvent,
}

impl AgentStreamMessageType {
    pub fn as_header_value(self) -> &'static str {
        match self {
            Self::ConversationMessage => "conversation_message",
            Self::PlatformEvent => "platform_event",
        }
    }

    pub fn webhook_event_name(self) -> &'static str {
        match self {
            Self::ConversationMessage => "execution_thread.message",
            Self::PlatformEvent => "execution_thread.platform_event",
        }
    }
}

impl FromStr for AgentStreamMessageType {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "conversation_message" => Ok(Self::ConversationMessage),
            "platform_event" => Ok(Self::PlatformEvent),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamEvent {
    PlatformEvent(String, serde_json::Value),
    ConversationMessage(ConversationRecord),
}

impl StreamEvent {
    pub fn message_type(&self) -> AgentStreamMessageType {
        match self {
            StreamEvent::PlatformEvent(_, _) => AgentStreamMessageType::PlatformEvent,
            StreamEvent::ConversationMessage(_) => AgentStreamMessageType::ConversationMessage,
        }
    }
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
