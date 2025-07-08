use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamEvent {
    Token(String),
    PlatformEvent(String, serde_json::Value),
    PlatformFunction(String, serde_json::Value),
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

impl<'de> Deserialize<'de> for TaskStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        Ok(match s.as_str() {
            "InProgress" => TaskStatus::InProgress,
            "Completed" => TaskStatus::Completed,
            "Failed" => TaskStatus::Failed,
            "Blocked" => TaskStatus::Blocked,
            _ => TaskStatus::Pending,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    #[serde(rename = "dependency")]
    pub dependencies: Option<Vec<String>>,
    pub context: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename = "task_plan")]
pub struct TaskPlan {
    #[serde(rename = "task")]
    pub tasks: Option<Vec<Task>>,
    pub reasoning: String,
    pub estimated_steps: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_name: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowCall {
    pub workflow_name: String,
    pub inputs: Value,
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
