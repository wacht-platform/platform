use std::time::Duration;

use models::{AgentHookStep, ConversationContent, ConversationMessageType};
use serde_json::{json, Value};
use tokio::time::timeout;

use super::core::AgentExecutor;
use crate::tools::external::VIRTUAL_TOOL_NAME_PREFIX;
use common::error::AppError;

const HOOK_STEP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecyclePhase {
    ExecutionStart,
    BeforeLlm,
    AfterLlm,
    BeforeTool,
    AfterTool,
    OnBudgetExhausted,
    ExecutionEnd,
}

impl LifecyclePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExecutionStart => "execution_start",
            Self::BeforeLlm => "before_llm",
            Self::AfterLlm => "after_llm",
            Self::BeforeTool => "before_tool",
            Self::AfterTool => "after_tool",
            Self::OnBudgetExhausted => "on_budget_exhausted",
            Self::ExecutionEnd => "execution_end",
        }
    }
}

fn hook_webhook_event(phase: LifecyclePhase, status: HookStepStatus) -> &'static str {
    match (phase, status) {
        (LifecyclePhase::ExecutionStart, HookStepStatus::Succeeded) => {
            "agent.execution_start_hook.succeeded"
        }
        (LifecyclePhase::ExecutionStart, HookStepStatus::Failed) => {
            "agent.execution_start_hook.failed"
        }
        (LifecyclePhase::ExecutionStart, HookStepStatus::Skipped) => {
            "agent.execution_start_hook.skipped"
        }
        (LifecyclePhase::BeforeLlm, HookStepStatus::Succeeded) => "agent.before_llm_hook.succeeded",
        (LifecyclePhase::BeforeLlm, HookStepStatus::Failed) => "agent.before_llm_hook.failed",
        (LifecyclePhase::BeforeLlm, HookStepStatus::Skipped) => "agent.before_llm_hook.skipped",
        (LifecyclePhase::AfterLlm, HookStepStatus::Succeeded) => "agent.after_llm_hook.succeeded",
        (LifecyclePhase::AfterLlm, HookStepStatus::Failed) => "agent.after_llm_hook.failed",
        (LifecyclePhase::AfterLlm, HookStepStatus::Skipped) => "agent.after_llm_hook.skipped",
        (LifecyclePhase::BeforeTool, HookStepStatus::Succeeded) => {
            "agent.before_tool_hook.succeeded"
        }
        (LifecyclePhase::BeforeTool, HookStepStatus::Failed) => "agent.before_tool_hook.failed",
        (LifecyclePhase::BeforeTool, HookStepStatus::Skipped) => "agent.before_tool_hook.skipped",
        (LifecyclePhase::AfterTool, HookStepStatus::Succeeded) => "agent.after_tool_hook.succeeded",
        (LifecyclePhase::AfterTool, HookStepStatus::Failed) => "agent.after_tool_hook.failed",
        (LifecyclePhase::AfterTool, HookStepStatus::Skipped) => "agent.after_tool_hook.skipped",
        (LifecyclePhase::OnBudgetExhausted, HookStepStatus::Succeeded) => {
            "agent.on_budget_exhausted_hook.succeeded"
        }
        (LifecyclePhase::OnBudgetExhausted, HookStepStatus::Failed) => {
            "agent.on_budget_exhausted_hook.failed"
        }
        (LifecyclePhase::OnBudgetExhausted, HookStepStatus::Skipped) => {
            "agent.on_budget_exhausted_hook.skipped"
        }
        (LifecyclePhase::ExecutionEnd, HookStepStatus::Succeeded) => {
            "agent.execution_end_hook.succeeded"
        }
        (LifecyclePhase::ExecutionEnd, HookStepStatus::Failed) => "agent.execution_end_hook.failed",
        (LifecyclePhase::ExecutionEnd, HookStepStatus::Skipped) => {
            "agent.execution_end_hook.skipped"
        }
    }
}

#[derive(Clone, Copy)]
enum HookStepStatus {
    Succeeded,
    Failed,
    Skipped,
}

impl HookStepStatus {
    fn as_str(self) -> &'static str {
        match self {
            HookStepStatus::Succeeded => "succeeded",
            HookStepStatus::Failed => "failed",
            HookStepStatus::Skipped => "skipped",
        }
    }
}

enum HookStepOutcome {
    Success(Value),
    Skipped { reason: &'static str },
}

impl AgentExecutor {
    pub(crate) async fn run_hooks(&mut self, kind: LifecyclePhase, extra: Value) {
        let steps: Vec<AgentHookStep> = match kind {
            LifecyclePhase::ExecutionStart => self.ctx.agent.hooks.execution_start.clone(),
            LifecyclePhase::BeforeLlm => self.ctx.agent.hooks.before_llm.clone(),
            LifecyclePhase::AfterLlm => self.ctx.agent.hooks.after_llm.clone(),
            LifecyclePhase::BeforeTool => self.ctx.agent.hooks.before_tool.clone(),
            LifecyclePhase::AfterTool => self.ctx.agent.hooks.after_tool.clone(),
            LifecyclePhase::OnBudgetExhausted => self.ctx.agent.hooks.on_budget_exhausted.clone(),
            LifecyclePhase::ExecutionEnd => self.ctx.agent.hooks.execution_end.clone(),
        };
        if steps.is_empty() {
            return;
        }

        let mut runtime_context = self.hook_runtime_context();
        if let (Value::Object(target), Value::Object(extra_map)) = (&mut runtime_context, extra) {
            for (k, v) in extra_map {
                target.insert(k, v);
            }
        }

        for (index, step) in steps.into_iter().enumerate() {
            let merged_input = merge_runtime_context(&step.args, &runtime_context);
            match self
                .run_hook_step(&step.tool_name, merged_input.clone())
                .await
            {
                Ok(HookStepOutcome::Success(output)) => {
                    if let Err(e) = self
                        .record_hook_success(&step.tool_name, &merged_input, &output)
                        .await
                    {
                        tracing::warn!(
                            hook = kind.as_str(),
                            tool = %step.tool_name,
                            error = %e,
                            "hook: failed to persist successful step result"
                        );
                    }
                    self.fire_hook_webhook(
                        kind,
                        HookStepStatus::Succeeded,
                        index,
                        &step.tool_name,
                        json!({ "input": merged_input, "output": output }),
                    )
                    .await;
                }
                Ok(HookStepOutcome::Skipped { reason }) => {
                    tracing::info!(
                        hook = kind.as_str(),
                        tool = %step.tool_name,
                        step = index,
                        reason,
                        "hook: step skipped"
                    );
                    self.fire_hook_webhook(
                        kind,
                        HookStepStatus::Skipped,
                        index,
                        &step.tool_name,
                        json!({ "reason": reason, "input": merged_input }),
                    )
                    .await;
                }
                Err(error) => {
                    let message = error.to_string();
                    tracing::warn!(
                        hook = kind.as_str(),
                        tool = %step.tool_name,
                        step = index,
                        error = %message,
                        "hook: step failed"
                    );
                    self.fire_hook_webhook(
                        kind,
                        HookStepStatus::Failed,
                        index,
                        &step.tool_name,
                        json!({ "error_message": message, "input": merged_input }),
                    )
                    .await;
                }
            }
        }
    }

    fn hook_runtime_context(&self) -> Value {
        json!({
            "agent_id": self.ctx.agent.id.to_string(),
            "deployment_id": self.ctx.agent.deployment_id.to_string(),
            "thread_id": self.ctx.thread_id.to_string(),
            "execution_run_id": self.ctx.execution_run_id.to_string(),
            "board_item_id": self.current_board_item_id().map(|id| id.to_string()),
        })
    }

    async fn run_hook_step(
        &self,
        tool_name: &str,
        input: Value,
    ) -> Result<HookStepOutcome, AppError> {
        let tool = self
            .ctx
            .agent
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .cloned()
            .or_else(|| {
                self.virtual_tool_cache
                    .get(&tool_lookup_id(self, tool_name))
                    .cloned()
            });

        let Some(tool) = tool else {
            if tool_name.starts_with(VIRTUAL_TOOL_NAME_PREFIX) {
                return Ok(HookStepOutcome::Skipped {
                    reason: "virtual tool not connected for this actor",
                });
            }
            return Err(AppError::BadRequest(format!(
                "Hook tool '{tool_name}' is not in the agent's catalog"
            )));
        };

        let request = AgentExecutor::build_tool_call_request(&tool, input)?;

        let exec_fut =
            self.tool_executor
                .execute_tool_request(&tool, &request, &self.filesystem, &self.shell);
        match timeout(HOOK_STEP_TIMEOUT, exec_fut).await {
            Ok(Ok(output)) => Ok(HookStepOutcome::Success(output)),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AppError::Internal(format!(
                "Hook tool '{tool_name}' timed out after {}s",
                HOOK_STEP_TIMEOUT.as_secs()
            ))),
        }
    }

    async fn record_hook_success(
        &mut self,
        tool_name: &str,
        input: &Value,
        output: &Value,
    ) -> Result<(), AppError> {
        self.store_conversation(
            ConversationContent::ToolResult {
                tool_name: tool_name.to_string(),
                status: "success".to_string(),
                input: input.clone(),
                output: Some(output.clone()),
                error: None,
            },
            ConversationMessageType::ToolResult,
        )
        .await
    }

    async fn fire_hook_webhook(
        &self,
        kind: LifecyclePhase,
        status: HookStepStatus,
        step_index: usize,
        tool_name: &str,
        extra: Value,
    ) {
        use commands::webhook_trigger::{TriggerWebhookEventCommand, console_webhook_app_slug};

        let event_name = hook_webhook_event(kind, status);

        let mut payload = json!({
            "agent_id": self.ctx.agent.id.to_string(),
            "deployment_id": self.ctx.agent.deployment_id.to_string(),
            "thread_id": self.ctx.thread_id.to_string(),
            "execution_run_id": self.ctx.execution_run_id.to_string(),
            "hook_kind": kind.as_str(),
            "status": status.as_str(),
            "tool_name": tool_name,
            "step_index": step_index,
            "timestamp": chrono::Utc::now(),
        });
        if let (Value::Object(target), Value::Object(extra_map)) = (&mut payload, extra) {
            for (k, v) in extra_map {
                target.insert(k, v);
            }
        }

        let console_id = crate::console_deployment_id();

        let trigger = TriggerWebhookEventCommand::new(
            console_id,
            console_webhook_app_slug(self.ctx.agent.deployment_id),
            event_name.to_string(),
            payload,
        );

        if let Err(e) = trigger
            .execute_with_deps(
                &common::deps::from_app(&self.ctx.app_state)
                    .db()
                    .redis()
                    .nats()
                    .id(),
            )
            .await
        {
            if !e.to_string().contains("Resource not found") {
                tracing::warn!(
                    hook = kind.as_str(),
                    tool = %tool_name,
                    error = %e,
                    "hook: failed to enqueue webhook event"
                );
            }
        }
    }
}

fn merge_runtime_context(args: &Value, runtime_context: &Value) -> Value {
    let mut merged = match args {
        Value::Object(_) => args.clone(),
        Value::Null => Value::Object(serde_json::Map::new()),
        other => json!({ "value": other }),
    };
    if let Value::Object(map) = &mut merged {
        map.insert("_runtime".to_string(), runtime_context.clone());
    }
    merged
}

fn tool_lookup_id(executor: &AgentExecutor, tool_name: &str) -> i64 {
    executor
        .virtual_tool_cache
        .iter()
        .find_map(|(id, tool)| (tool.name == tool_name).then_some(*id))
        .unwrap_or(i64::MIN)
}
