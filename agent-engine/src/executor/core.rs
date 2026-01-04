use crate::context::ContextOrchestrator;
use crate::tools::ToolExecutor;
use crate::filesystem::{AgentFilesystem, shell::ShellExecutor};

use common::error::AppError;
use common::state::AppState;
use dto::json::agent_executor::{
    ConversationInsights, ObjectiveDefinition, TaskExecutionResult,
};
use dto::json::StreamEvent;
use models::{
    AgentExecutionState, AiAgentWithFeatures, ConversationRecord, ExecutionContextStatus, MemoryRecord, WorkflowExecutionState,
};
use models::{AiTool, AiToolConfiguration, AiToolType, InternalToolConfiguration, InternalToolType, SchemaField};
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
    pub(super) filesystem: AgentFilesystem,
    pub(super) shell: ShellExecutor,
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
            ToolExecutor::new(self.app_state.clone(), self.agent.clone(), self.context_id).with_channel(self.channel.clone());
        let context_orchestrator =
            ContextOrchestrator::new(self.app_state.clone(), self.agent.clone(), self.context_id);

        let execution_id = self.app_state.sf.next_id()?.to_string();
        
        let filesystem = AgentFilesystem::new(
            &self.agent.deployment_id.to_string(),
            &self.agent.id.to_string(),
            &self.context_id.to_string(),
            &execution_id,
        );
        
        if let Err(e) = filesystem.initialize().await {
            tracing::warn!("Failed to initialize agent filesystem: {}", e);
        }

        let shell = ShellExecutor::new(filesystem.execution_root());
        
        for kb in &self.agent.knowledge_bases {
            if let Err(e) = filesystem.link_knowledge_base(&kb.id.to_string(), &kb.name).await {
                tracing::warn!("Failed to link knowledge base {} ({}): {}", kb.name, kb.id, e);
            }
        }
        
        let internal_tools = vec![
            (
                "read_file",
                "Read file content. Supports line ranges. Returns total_lines for navigation.",
                InternalToolType::ReadFile,
                vec![
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Path to the file".to_string()),
                        required: true,
                    },
                    SchemaField {
                        name: "start_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Start line (1-indexed, optional)".to_string()),
                        required: false,
                    },
                    SchemaField {
                        name: "end_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("End line (inclusive, optional)".to_string()),
                        required: false,
                    }
                ]
            ),
            (
                "write_file",
                "Write to file. For partial writes (with start_line/end_line), must read_file first.",
                InternalToolType::WriteFile,
                vec![
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Path to write (memory/, workspace/, scratch/ only)".to_string()),
                        required: true,
                    },
                    SchemaField {
                        name: "content".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Content to write".to_string()),
                        required: true,
                    },
                    SchemaField {
                        name: "start_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Replace from this line (1-indexed). Requires prior read_file.".to_string()),
                        required: false,
                    },
                    SchemaField {
                        name: "end_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Replace up to this line (inclusive). Requires prior read_file.".to_string()),
                        required: false,
                    }
                ]
            ),
            (
                "list_directory",
                "List files and directories at a path.",
                InternalToolType::ListDirectory,
                vec![
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Directory path (default: '/')".to_string()),
                        required: false,
                    }
                ]
            ),
            (
                "search_files",
                "Search for text patterns in files.",
                InternalToolType::SearchFiles,
                vec![
                    SchemaField {
                        name: "query".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Text or regex to search for".to_string()),
                        required: true,
                    },
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Directory to search (default: '/')".to_string()),
                        required: false,
                    }
                ]
            ),
            (
                "execute_command",
                "Execute a shell command. Allowed commands: cat, head, tail, grep, rg, find, ls, tree, wc, du, df, touch, mkdir, echo, cp, mv, rm, chmod, sed, awk, sort, uniq, jq, cut, tr, diff, date, whoami, pwd, printf.",
                InternalToolType::ExecuteCommand,
                vec![
                    SchemaField {
                        name: "command".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Shell command to run".to_string()),
                        required: true,
                    }
                ]
            ),
            (
                "save_memory",
                "Save important information to long-term memory. Use for facts, preferences, procedures that should be remembered across sessions.",
                InternalToolType::SaveMemory,
                vec![
                    SchemaField {
                        name: "content".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("The information to remember".to_string()),
                        required: true,
                    },
                    SchemaField {
                        name: "category".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Category: procedural (how-to), semantic (facts), episodic (events), working (temp)".to_string()),
                        required: true,
                    },
                    SchemaField {
                        name: "importance".to_string(),
                        field_type: "NUMBER".to_string(),
                        description: Some("Importance 0.0-1.0 (default: 0.5)".to_string()),
                        required: false,
                    }
                ]
            ),
        ];

        let mut current_tools = self.agent.tools.clone();
        for (name, desc, tool_type, schema) in internal_tools {
            if !current_tools.iter().any(|t| t.name == name) {
                current_tools.push(AiTool {
                    id: -1,
                    name: name.to_string(),
                    description: Some(desc.to_string()),
                    tool_type: AiToolType::Internal,
                    deployment_id: self.agent.deployment_id,
                    configuration: AiToolConfiguration::Internal(
                        InternalToolConfiguration {
                            tool_type,
                            input_schema: Some(schema),
                        }
                    ),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                });
            }
        }
        
        let mut agent_with_tools = self.agent.clone();
        agent_with_tools.tools = current_tools;

        let mut executor = AgentExecutor {
            agent: agent_with_tools.clone(),
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
            filesystem,
            shell,
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
