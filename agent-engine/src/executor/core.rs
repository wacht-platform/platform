use crate::context::ContextOrchestrator;
use crate::tools::ToolExecutor;

use common::error::AppError;
use common::state::AppState;
use dto::json::agent_executor::{
    ConversationInsights, ObjectiveDefinition, TaskExecutionResult,
};
use dto::json::StreamEvent;
use models::{
    AgentExecutionState, AiAgentWithFeatures, ConversationRecord, ExecutionContextStatus, MemoryRecord, WorkflowExecutionState,
};
use queries::{GetExecutionContextQuery, Query};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum ResumeContext {
    PlatformFunction(String, Value),
    UserInput(String),
}

pub struct AgentExecutor {
    pub(super) agent: AiAgentWithFeatures,
    pub(super) app_state: AppState,
    pub(super) context_id: i64,
    pub(super) conversations: Vec<ConversationRecord>,
    pub(super) context_orchestrator: ContextOrchestrator,
    pub(super) tool_executor: ToolExecutor,
    pub(super) channel: tokio::sync::mpsc::Sender<StreamEvent>,
    pub(super) memories: Vec<MemoryRecord>,
    pub(super) loaded_memory_ids: std::collections::HashSet<i64>,
    pub(super) user_request: String,
    pub(super) current_objective: Option<ObjectiveDefinition>,
    pub(super) conversation_insights: Option<ConversationInsights>,
    pub(super) task_results: HashMap<String, TaskExecutionResult>,
    pub(super) current_workflow_id: Option<i64>,
    pub(super) current_workflow_state: Option<HashMap<String, Value>>,
    pub(super) current_workflow_node_id: Option<String>,
    pub(super) current_workflow_execution_path: Vec<String>,
    pub(super) system_instructions: Option<String>,
}

pub struct AgentExecutorBuilder {
    agent: AiAgentWithFeatures,
    app_state: AppState,
    context_id: i64,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
}

impl AgentExecutorBuilder {
    pub fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            agent,
            context_id,
            app_state,
            channel,
        }
    }

    pub async fn build(self) -> Result<AgentExecutor, AppError> {
        let tool_executor =
            ToolExecutor::new(self.app_state.clone()).with_channel(self.channel.clone());
        let context_orchestrator =
            ContextOrchestrator::new(self.app_state.clone(), self.agent.clone(), self.context_id);

        let mut executor = AgentExecutor {
            agent: self.agent.clone(),
            app_state: self.app_state.clone(),
            context_id: self.context_id,
            context_orchestrator,
            tool_executor,
            user_request: String::new(),
            channel: self.channel,
            memories: Vec::new(),
            loaded_memory_ids: std::collections::HashSet::new(),
            conversations: Vec::new(),
            current_objective: None,
            conversation_insights: None,
            task_results: HashMap::new(),
            current_workflow_id: None,
            current_workflow_state: None,
            current_workflow_node_id: None,
            current_workflow_execution_path: Vec::new(),
            system_instructions: None,
        };

        let context = GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .execute(&self.app_state)
            .await?;

        executor.system_instructions = context.system_instructions;

        if context.status == ExecutionContextStatus::WaitingForInput {
            if let Some(state) = context.execution_state {
                executor.restore_from_state(state)?;
            }
        }

        Ok(executor)
    }
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(agent, context_id, app_state, channel)
            .build()
            .await
    }

    pub(super) fn restore_from_state(&mut self, state: AgentExecutionState) -> Result<(), AppError> {
        self.task_results = state
            .task_results
            .into_iter()
            .filter_map(|(k, v)| {
                serde_json::from_value::<TaskExecutionResult>(v)
                    .ok()
                    .map(|result| (k, result))
            })
            .collect();

        if let Some(objective) = state.current_objective {
            self.current_objective = serde_json::from_value(objective).ok();
        }

        if let Some(insights) = state.conversation_insights {
            self.conversation_insights = serde_json::from_value(insights).ok();
        }

        if let Some(workflow_state) = state.workflow_state {
            self.current_workflow_id = Some(workflow_state.workflow_id);
            self.current_workflow_state = Some(workflow_state.workflow_state);
            self.current_workflow_node_id = Some(workflow_state.current_node_id);
            self.current_workflow_execution_path = workflow_state.execution_path;
        }

        Ok(())
    }

    pub(super) fn get_current_workflow_state(&self) -> Option<WorkflowExecutionState> {
        match (
            self.current_workflow_id,
            &self.current_workflow_state,
            &self.current_workflow_node_id,
        ) {
            (Some(workflow_id), Some(workflow_state), Some(node_id)) => {
                Some(WorkflowExecutionState {
                    workflow_id,
                    workflow_state: workflow_state.clone(),
                    current_node_id: node_id.clone(),
                    execution_path: self.current_workflow_execution_path.clone(),
                })
            }
            _ => None,
        }
    }
}
