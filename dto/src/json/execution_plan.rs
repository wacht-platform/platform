use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "execution_plan")]
pub struct ExecutionPlan {
    pub message: String,
    pub analysis: PlanAnalysis,
    #[serde(rename = "task")]
    pub tasks: Vec<PlannedTask>,
    pub success_criteria: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "planning_iteration_response")]
pub struct PlanningIterationResponse {
    pub iteration_notes: String,
    pub needs_more_iteration: bool,
    #[serde(default)]
    pub context_search_request: Option<String>,
    pub execution_plan: ExecutionPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanAnalysis {
    pub understanding: String,
    pub challenge: String,
    pub approach: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedTask {
    pub id: String,
    pub objective: String,
    pub requirements: TaskRequirements,
    pub expected_output: String,
    #[serde(default)]
    pub dependencies: TaskDependencies,
    pub priority: TaskPriority,
    pub failure_strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequirements {
    #[serde(rename = "requirement")]
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TaskDependencies {
    pub dependencies: Vec<String>,
}

impl<'de> Deserialize<'de> for TaskDependencies {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            #[serde(rename = "dependency", default)]
            dependencies: Vec<String>,
        }

        // Try to deserialize as Helper struct
        let helper = Helper::deserialize(deserializer).unwrap_or(Helper {
            dependencies: Vec::new(),
        });

        Ok(TaskDependencies {
            dependencies: helper.dependencies,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "task_execution")]
pub struct TaskExecution {
    pub approach: String,
    pub actions: ActionsList,
    pub expected_result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionsList {
    #[serde(rename = "action")]
    pub actions: Vec<ExecutionAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAction {
    #[serde(rename = "type")]
    pub action_type: ActionType,
    pub details: serde_json::Value,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    ToolCall,
    KnowledgeSearch,
    MemoryOperation,
    ContextSearch,
    MessageUser,
}

impl PlannedTask {
    pub fn to_task(&self) -> super::agent::Task {
        let deps = &self.dependencies.dependencies;
        super::agent::Task {
            id: self.id.clone(),
            description: self.objective.clone(),
            status: super::agent::TaskStatus::Pending,
            dependencies: if deps.is_empty() {
                None
            } else {
                Some(deps.clone())
            },
            context: serde_json::json!({
                "type": "planned_execution",
                "objective": self.objective,
                "requirements": self.requirements.requirements,
                "expected_output": self.expected_output,
                "priority": self.priority,
                "failure_strategy": self.failure_strategy,
            }),
            result: None,
            error: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}
