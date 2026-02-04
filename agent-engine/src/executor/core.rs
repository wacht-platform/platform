use crate::context::ContextOrchestrator;
use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use crate::tools::ToolExecutor;

use common::error::AppError;
use dto::json::agent_executor::{ConversationInsights, ObjectiveDefinition, TaskExecutionResult};
use dto::json::StreamEvent;
use models::{
    AgentExecutionState, ConversationRecord, ExecutionContextStatus,
    MemoryRecord,
};
use models::{
    AiTool, AiToolConfiguration, AiToolType, InternalToolConfiguration, UseExternalServiceToolConfiguration, UseExternalServiceToolType,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum ResumeContext {
    PlatformFunction(String, serde_json::Value),
    UserInput(String),
}

pub struct AgentExecutor {
    pub(super) ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
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
    pub(super) system_instructions: Option<String>,
    pub(super) filesystem: AgentFilesystem,
    pub(super) shell: ShellExecutor,
}


pub struct AgentExecutorBuilder {
    ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
}

impl AgentExecutorBuilder {
    pub fn new(
        ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            ctx,
            channel,
        }
    }

    pub async fn build(self) -> Result<AgentExecutor, AppError> {
        let execution_context = self.ctx.clone();

        let tool_executor = ToolExecutor::new(execution_context.clone())
            .with_channel(self.channel.clone());
        let context_orchestrator = ContextOrchestrator::new(execution_context.clone());

        let execution_id = self.ctx.app_state.sf.next_id()?.to_string();

        let filesystem = AgentFilesystem::new(
            &self.ctx.agent.deployment_id.to_string(),
            &self.ctx.agent.id.to_string(),
            &self.ctx.context_id.to_string(),
            &execution_id,
        );

        if let Err(e) = filesystem.initialize().await {
            tracing::warn!("Failed to initialize agent filesystem: {}", e);
        }

        let shell = ShellExecutor::new(filesystem.execution_root());

        for kb in &self.ctx.agent.knowledge_bases {
            if let Err(e) = filesystem
                .link_knowledge_base(&kb.id.to_string(), &kb.name)
                .await
            {
                tracing::warn!(
                    "Failed to link knowledge base {} ({}): {}",
                    kb.name,
                    kb.id,
                    e
                );
            }
        }

        let internal_tools = super::tool_definitions::internal_tools();

        let context = execution_context.get_context().await?;

        // Get integration status from cached context
        let integration_status = execution_context.integration_status().await?;

        // Link teams activity directory if Teams is enabled
        if integration_status.teams_enabled {
            if let Some(context_group) = &context.context_group {
                if let Err(e) = filesystem.link_teams_activity(context_group).await {
                    tracing::warn!("Failed to link teams-activity directory: {}", e);
                }
            }
        }

        let mut current_tools = self.ctx.agent.tools.clone();
        for (name, desc, tool_type, schema) in internal_tools {
            if !current_tools.iter().any(|t| t.name == name) {
                current_tools.push(AiTool {
                    id: -1,
                    name: name.to_string(),
                    description: Some(desc.to_string()),
                    tool_type: AiToolType::Internal,
                    deployment_id: self.ctx.agent.deployment_id,
                    configuration: AiToolConfiguration::Internal(InternalToolConfiguration {
                        tool_type,
                        input_schema: Some(schema),
                    }),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                });
            }
        }

        if integration_status.teams_enabled {
            let teams_tools = super::tool_definitions::teams_tools();

            for (name, desc, service_type, schema) in teams_tools {
                if !current_tools.iter().any(|t| t.name == name) {
                    current_tools.push(AiTool {
                        id: -1,
                        name: name.to_string(),
                        description: Some(desc.to_string()),
                        tool_type: AiToolType::UseExternalService,
                        deployment_id: self.ctx.agent.deployment_id,
                        configuration: AiToolConfiguration::UseExternalService(
                            UseExternalServiceToolConfiguration {
                                service_type,
                                input_schema: Some(schema),
                            },
                        ),
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    });
                }
            }
        }

        if integration_status.clickup_enabled {
            let clickup_tools = super::tool_definitions::clickup_tools();

            for (name, desc, service_type, schema) in clickup_tools {
                if !current_tools.iter().any(|t| t.name == name) {
                    current_tools.push(AiTool {
                        id: -1,
                        name: name.to_string(),
                        description: Some(desc.to_string()),
                        tool_type: AiToolType::UseExternalService,
                        deployment_id: self.ctx.agent.deployment_id,
                        configuration: AiToolConfiguration::UseExternalService(
                            UseExternalServiceToolConfiguration {
                                service_type,
                                input_schema: Some(schema),
                            },
                        ),
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    });
                }
            }
        }

        if !current_tools
            .iter()
            .any(|t| t.name == "spawn_context_execution")
        {
            current_tools.push(AiTool {
                id: -1,
                name: "spawn_context_execution".to_string(),
                description: Some(
                    "Spawn a new agent execution in another context. Just like you are currently running in your context with your own conversation history and tools - this will start a separate, self-contained agent instance in the target context. The spawned instance will receive your message as input and operate independently with its own context, history, and available tools. Use this to delegate tasks, notify other channels, or hand off work.".to_string()
                ),
                tool_type: AiToolType::UseExternalService,
                deployment_id: self.ctx.agent.deployment_id,
                configuration: AiToolConfiguration::UseExternalService(
                    UseExternalServiceToolConfiguration {
                        service_type: UseExternalServiceToolType::TriggerContext,
                        input_schema: Some(crate::executor::tool_definitions::spawn_context_execution_schema()),
                    }
                ),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
        }

        let mut agent_with_tools = self.ctx.agent.clone();
        agent_with_tools.tools = current_tools.clone();

        let execution_context = execution_context.with_agent(agent_with_tools);

        let mut executor = AgentExecutor {
            ctx: execution_context,
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
            system_instructions: None,
            filesystem,
            shell,
        };

        executor.system_instructions = context.system_instructions.clone();

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
        ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(ctx, channel)
            .build()
            .await
    }

    pub(super) fn restore_from_state(
        &mut self,
        state: AgentExecutionState,
    ) -> Result<(), AppError> {
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

        Ok(())
    }
}
