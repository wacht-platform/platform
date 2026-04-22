use crate::runtime::thread_execution_context::DeploymentProviderKeys;
use crate::{AgentExecutor, ResumeContext};
use common::error::AppError;
use common::state::AppState;
use dto::json::StreamEvent;
use futures::StreamExt;
use models::AgentThreadStatus;
use models::AiAgentWithFeatures;
use models::ThreadEvent;

pub struct AgentHandler {
    app_state: AppState,
}

enum ExecutionMode {
    ApprovalResponse(Vec<dto::json::deployment::ToolApprovalSelection>),
    Conversation(i64),
    ThreadEvent(ThreadEvent),
}

fn thread_owns_status(thread_purpose: &str) -> bool {
    matches!(
        thread_purpose,
        models::agent_thread::purpose::CONVERSATION | models::agent_thread::purpose::COORDINATOR
    )
}

fn assignment_id_from_thread_event(thread_event: &ThreadEvent) -> Option<i64> {
    thread_event
        .assignment_execution_payload()
        .map(|payload| payload.assignment_id)
}

fn assignment_completion_note(
    context: &models::AgentThreadState,
    error: Option<&AppError>,
) -> Option<String> {
    if let Some(note) = context
        .execution_state
        .as_ref()
        .and_then(|state| state.assignment_outcome_override.as_ref())
        .and_then(|override_state| override_state.note.as_deref())
    {
        let trimmed = note.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    match error {
        Some(AppError::Database(_)) => None,
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

#[derive(Clone)]
struct AssignmentCompletion {
    assignment_id: i64,
    assignment_status: String,
    result_status: Option<String>,
    note: Option<String>,
    should_clear_override: bool,
}

fn infer_assignment_status_from_execution(
    execution_result: &Result<(), AppError>,
    thread_status: &models::AgentThreadStatus,
) -> Option<String> {
    match (execution_result, thread_status) {
        (Ok(()), models::AgentThreadStatus::Completed | models::AgentThreadStatus::Idle) => {
            Some(models::project_task_board::assignment_status::COMPLETED.to_string())
        }
        (Ok(()), models::AgentThreadStatus::Failed) | (Err(_), _) => {
            Some(models::project_task_board::assignment_status::BLOCKED.to_string())
        }
        _ => None,
    }
}

fn default_assignment_result_status(assignment_status: &str) -> Option<String> {
    match assignment_status {
        models::project_task_board::assignment_status::COMPLETED => {
            Some(models::project_task_board::assignment_result_status::COMPLETED.to_string())
        }
        models::project_task_board::assignment_status::BLOCKED => {
            Some(models::project_task_board::assignment_result_status::BLOCKED.to_string())
        }
        models::project_task_board::assignment_status::REJECTED => {
            Some(models::project_task_board::assignment_result_status::REJECTED.to_string())
        }
        models::project_task_board::assignment_status::CANCELLED => {
            Some(models::project_task_board::assignment_result_status::CANCELLED.to_string())
        }
        _ => None,
    }
}

#[derive(Clone)]
pub struct ExecutionRequest {
    pub agent: AiAgentWithFeatures,
    pub conversation_id: Option<i64>,
    pub thread_id: i64,
    pub thread_event_id: Option<i64>,
    pub execution_run_id: i64,
    pub execution_token: String,
    pub approval_response: Option<Vec<dto::json::deployment::ToolApprovalSelection>>,
    pub thread_event: Option<ThreadEvent>,
}

impl AgentHandler {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn execute_agent_streaming(&self, request: ExecutionRequest) -> Result<(), AppError> {
        let (sender, receiver) = tokio::sync::mpsc::channel::<StreamEvent>(100);
        let thread_key = request.thread_id.to_string();
        let deployment_id = request.agent.deployment_id;

        let deployment_ai_settings = queries::GetDeploymentAiSettingsQuery::new(deployment_id)
            .execute_with_db(self.app_state.db_router.writer())
            .await
            .ok()
            .flatten();

        let provider_keys = DeploymentProviderKeys::from_settings(
            deployment_ai_settings.as_ref(),
            &self.app_state.encryption_service,
        )?;

        let execution_context =
            crate::runtime::thread_execution_context::ThreadExecutionContext::new(
                self.app_state.clone(),
                request.agent.clone(),
                request.thread_id,
                request.execution_run_id,
                provider_keys,
            );

        self.spawn_message_publisher(receiver, thread_key.clone(), execution_context.clone());

        let thread_state = execution_context.get_thread().await?;

        let execution_context_for_notification = execution_context.clone();

        let mut executor = AgentExecutor::new(execution_context, sender).await?;

        let execution_mode = match (
            request.conversation_id,
            request.approval_response.clone(),
            request.thread_event.clone(),
        ) {
            (_, Some(approvals), _) => ExecutionMode::ApprovalResponse(approvals),
            (Some(conv_id), None, None) => ExecutionMode::Conversation(conv_id),
            (None, None, Some(thread_event)) => ExecutionMode::ThreadEvent(thread_event),
            _ => {
                return Err(AppError::Internal(
                    "Invalid execution request: conversation_id, approval_response, or thread_event required"
                        .to_string(),
                ));
            }
        };

        let execution_result = self
            .run_execution_mode(
                &thread_key,
                &request.execution_token,
                &mut executor,
                execution_mode,
                thread_state.id,
                deployment_id,
            )
            .await;

        if execution_result.is_err() && thread_owns_status(&thread_state.thread_purpose) {
            let _ = commands::UpdateAgentThreadStateCommand::new(thread_state.id, deployment_id)
                .with_status(models::AgentThreadStatus::Failed)
                .execute_with_deps(&common::deps::from_app(&self.app_state).db().nats().id())
                .await;
        }

        self.finalize_execution(
            &execution_context_for_notification,
            &request,
            deployment_id,
            &execution_result,
        )
        .await;

        if !request.execution_token.is_empty() {
            if let Err(e) = clear_execution_token_if_current(
                &thread_key,
                &request.execution_token,
                &self.app_state,
            )
            .await
            {
                let _ = e;
            }
        }

        execution_result
    }

    fn spawn_message_publisher(
        &self,
        mut receiver: tokio::sync::mpsc::Receiver<StreamEvent>,
        thread_key: String,
        ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    ) {
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                let _ = publish_stream_event(ctx.clone(), &thread_key, message).await;
            }
        });
    }

    async fn run_execution_mode(
        &self,
        thread_key: &str,
        execution_token: &str,
        agent_executor: &mut AgentExecutor,
        execution_mode: ExecutionMode,
        thread_id: i64,
        deployment_id: i64,
    ) -> Result<(), AppError> {
        match execution_mode {
            ExecutionMode::ApprovalResponse(approvals) => {
                self.run_with_execution_watch(
                    thread_key,
                    execution_token,
                    thread_id,
                    deployment_id,
                    agent_executor.resume_execution(ResumeContext::ApprovalResponse(approvals)),
                )
                .await
            }
            ExecutionMode::Conversation(conversation_id) => {
                self.run_with_execution_watch(
                    thread_key,
                    execution_token,
                    thread_id,
                    deployment_id,
                    agent_executor.execute_with_conversation_id(conversation_id),
                )
                .await
            }
            ExecutionMode::ThreadEvent(thread_event) => {
                if let Some(assignment_id) = assignment_id_from_thread_event(&thread_event) {
                    let update_deps = common::deps::from_app(&self.app_state).db().nats().id();
                    commands::UpdateProjectTaskBoardItemAssignmentStateCommand::new(
                        assignment_id,
                        models::project_task_board::assignment_status::IN_PROGRESS.to_string(),
                    )
                    .with_note("Assignment execution started".to_string())
                    .execute_with_deps(&update_deps)
                    .await?;
                }

                self.run_with_execution_watch(
                    thread_key,
                    execution_token,
                    thread_id,
                    deployment_id,
                    agent_executor.execute_with_thread_event(thread_event),
                )
                .await
            }
        }
    }

    async fn run_with_execution_watch<F>(
        &self,
        thread_key: &str,
        execution_token: &str,
        thread_id: i64,
        deployment_id: i64,
        execution_future: F,
    ) -> Result<(), AppError>
    where
        F: std::future::Future<Output = Result<(), AppError>>,
    {
        if execution_token.is_empty() {
            let _ = (thread_id, deployment_id);
            return execution_future.await;
        }
        tokio::pin!(execution_future);
        let mut token_watch =
            watch_execution_token_changes(&self.app_state, thread_key, execution_token).await?;

        loop {
            tokio::select! {
                result = &mut execution_future => {
                    return result;
                }
                next = token_watch.next() => {
                    match next {
                        Some(Ok(entry)) => {
                            if entry.value.as_ref() != execution_token.as_bytes() {
                                mark_thread_interrupted(&self.app_state, thread_id, deployment_id).await;
                                return Ok(());
                            }
                        }
                        Some(Err(error)) => {
                            return Err(AppError::Internal(format!(
                                "Failed to watch execution token for thread {}: {}",
                                thread_key, error
                            )));
                        }
                        None => {
                            if execution_token_superseded(&self.app_state, thread_key, execution_token).await? {
                                mark_thread_interrupted(&self.app_state, thread_id, deployment_id).await;
                                return Ok(());
                            }
                            return execution_future.await;
                        }
                    }
                }
            }
        }
    }

    async fn finalize_execution(
        &self,
        execution_context: &std::sync::Arc<
            crate::runtime::thread_execution_context::ThreadExecutionContext,
        >,
        request: &ExecutionRequest,
        deployment_id: i64,
        execution_result: &Result<(), AppError>,
    ) {
        execution_context.invalidate_cache();
        let Ok(context) = execution_context.get_thread().await else {
            return;
        };

        self.finalize_assignment_outcome(&context, request, deployment_id, execution_result)
            .await;
        self.finalize_execution_run(&context, request, deployment_id, execution_result)
            .await;
        self.finalize_thread_event(request, execution_result).await;
        self.schedule_follow_up_work(&context, deployment_id).await;
    }

    async fn finalize_assignment_outcome(
        &self,
        context: &models::AgentThreadState,
        request: &ExecutionRequest,
        deployment_id: i64,
        execution_result: &Result<(), AppError>,
    ) {
        let Some(completion) =
            self.resolve_assignment_completion(context, request, execution_result)
        else {
            return;
        };

        let update_deps = common::deps::from_app(&self.app_state).db().nats().id();
        let mut command = commands::UpdateProjectTaskBoardItemAssignmentStateCommand::new(
            completion.assignment_id,
            completion.assignment_status,
        )
        .with_result(completion.result_status, completion.note.clone(), None);
        if let Some(note) = completion.note {
            command = command.with_note(note);
        }

        let assignment_update_result = command.execute_with_deps(&update_deps).await;
        if assignment_update_result.is_ok() && completion.should_clear_override {
            self.clear_assignment_outcome_override(context, deployment_id, &update_deps)
                .await;
        }
    }

    fn resolve_assignment_completion(
        &self,
        context: &models::AgentThreadState,
        request: &ExecutionRequest,
        execution_result: &Result<(), AppError>,
    ) -> Option<AssignmentCompletion> {
        let assignment_id = request
            .thread_event
            .as_ref()
            .and_then(assignment_id_from_thread_event)?;

        let assignment_override = context
            .execution_state
            .as_ref()
            .and_then(|state| state.assignment_outcome_override.clone());

        let assignment_status = assignment_override
            .as_ref()
            .map(|override_value| override_value.assignment_status.clone())
            .or_else(|| {
                infer_assignment_status_from_execution(execution_result, &context.status)
            })?;

        let result_status = assignment_override
            .as_ref()
            .and_then(|override_value| override_value.result_status.clone())
            .or_else(|| default_assignment_result_status(&assignment_status));

        Some(AssignmentCompletion {
            assignment_id,
            assignment_status,
            result_status,
            note: assignment_completion_note(context, execution_result.as_ref().err()),
            should_clear_override: assignment_override.is_some(),
        })
    }

    async fn clear_assignment_outcome_override<D>(
        &self,
        context: &models::AgentThreadState,
        deployment_id: i64,
        update_deps: &D,
    ) where
        D: common::HasDbRouter + common::HasNatsJetStreamProvider + common::HasIdProvider,
    {
        if let Some(mut state) = context.execution_state.clone() {
            state.assignment_outcome_override = None;
            let _ = commands::UpdateAgentThreadStateCommand::new(context.id, deployment_id)
                .with_execution_state(state)
                .execute_with_deps(update_deps)
                .await;
        }
    }

    async fn finalize_execution_run(
        &self,
        context: &models::AgentThreadState,
        request: &ExecutionRequest,
        deployment_id: i64,
        execution_result: &Result<(), AppError>,
    ) {
        let run_status = match execution_result {
            Ok(()) => context.status.to_string(),
            Err(_) => "failed".to_string(),
        };

        let mut run_update =
            commands::UpdateExecutionRunStateCommand::new(request.execution_run_id, deployment_id)
                .with_status(run_status);
        if execution_result.is_ok() {
            match context.status {
                models::AgentThreadStatus::Completed => {
                    run_update = run_update.mark_completed();
                }
                models::AgentThreadStatus::Failed => {
                    run_update = run_update.mark_failed();
                }
                _ => {}
            }
        }

        let _ = run_update
            .execute_with_db(self.app_state.db_router.writer())
            .await;
    }

    async fn finalize_thread_event(
        &self,
        request: &ExecutionRequest,
        execution_result: &Result<(), AppError>,
    ) {
        let Some(thread_event_id) = request.thread_event_id else {
            return;
        };

        let terminal_status = match execution_result {
            Ok(()) => models::thread_event::status::COMPLETED.to_string(),
            Err(_) => models::thread_event::status::FAILED.to_string(),
        };
        let mut update_event =
            commands::UpdateThreadEventStateCommand::new(thread_event_id, terminal_status)
                .with_caused_by_run_id(request.execution_run_id);
        update_event = if execution_result.is_ok() {
            update_event.mark_completed()
        } else {
            update_event.mark_failed()
        };
        let _ = update_event
            .execute_with_db(self.app_state.db_router.writer())
            .await;
    }

    async fn schedule_follow_up_work(
        &self,
        context: &models::AgentThreadState,
        deployment_id: i64,
    ) {
        if context.status == AgentThreadStatus::Running {
            return;
        }

        let publish_deps = common::deps::from_app(&self.app_state).nats().id();
        let _ = commands::PublishThreadScheduleCommand::new(deployment_id, context.id)
            .execute_with_deps(&publish_deps)
            .await;
    }
}

async fn publish_stream_event(
    ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    thread_key: &str,
    event: StreamEvent,
) -> Result<(), AppError> {
    let app_state = &ctx.app_state;
    let deployment_id = ctx.agent.deployment_id;
    let jetstream = &app_state.nats_jetstream;
    let subject = format!("agent_execution_stream.thread:{thread_key}");

    let (message_type, payload) = dto::json::encode_stream_event(&event)
        .map_err(|e| AppError::Internal(format!("Failed to encode stream event: {e}")))?;

    let mut headers = async_nats::HeaderMap::new();
    headers.insert("message_type", message_type.as_header_value());
    headers.insert("thread_id", thread_key);
    headers.insert("deployment_id", deployment_id.to_string().as_str());

    jetstream
        .publish_with_headers(subject.clone(), headers, payload.clone().into())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {e}")))?;

    use commands::webhook_trigger::TriggerWebhookEventCommand;

    let webhook_payload = serde_json::json!({
        "thread_id": thread_key,
        "message_type": message_type.as_header_value(),
        "data": serde_json::from_slice::<serde_json::Value>(&payload).unwrap_or(serde_json::Value::Null),
        "timestamp": chrono::Utc::now(),
    });

    let console_id = std::env::var("CONSOLE_DEPLOYMENT_ID")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .unwrap_or(0);

    let trigger_command = TriggerWebhookEventCommand::new(
        console_id,
        deployment_id.to_string(),
        message_type.webhook_event_name().to_string(),
        webhook_payload,
    );

    if let Err(e) = trigger_command
        .execute_with_deps(&common::deps::from_app(app_state).db().redis().nats().id())
        .await
    {
        if !e.to_string().contains("Resource not found") {
            let _ = e;
        }
    }

    Ok(())
}

async fn watch_execution_token_changes(
    app_state: &AppState,
    thread_key: &str,
    current_execution_token: &str,
) -> Result<async_nats::jetstream::kv::Watch, AppError> {
    let kv = app_state
        .nats_jetstream
        .get_key_value("agent_execution_kv")
        .await
        .map_err(|error| {
            AppError::Internal(format!(
                "Failed to access execution token KV bucket for thread {}: {}",
                thread_key, error
            ))
        })?;

    let mut watch = kv.watch_with_history(thread_key).await.map_err(|error| {
        AppError::Internal(format!(
            "Failed to create execution token watch for thread {}: {}",
            thread_key, error
        ))
    })?;

    while let Some(entry) = watch.next().await {
        let entry = entry.map_err(|error| {
            AppError::Internal(format!(
                "Failed to initialize execution token watch for thread {}: {}",
                thread_key, error
            ))
        })?;

        if entry.seen_current && entry.value.as_ref() == current_execution_token.as_bytes() {
            break;
        }

        if entry.seen_current {
            break;
        }
    }

    Ok(watch)
}

async fn clear_execution_token_if_current(
    thread_key: &str,
    current_execution_token: &str,
    app_state: &AppState,
) -> Result<(), async_nats::Error> {
    let kv = app_state
        .nats_jetstream
        .get_key_value("agent_execution_kv")
        .await?;
    match kv.get(thread_key).await? {
        Some(entry) if entry.as_ref() == current_execution_token.as_bytes() => {
            kv.delete(thread_key).await?;
        }
        _ => {}
    }
    Ok(())
}

async fn execution_token_superseded(
    app_state: &AppState,
    thread_key: &str,
    current_execution_token: &str,
) -> Result<bool, AppError> {
    let kv = match app_state
        .nats_jetstream
        .get_key_value("agent_execution_kv")
        .await
    {
        Ok(store) => store,
        Err(_) => return Ok(false),
    };

    let current = match kv.get(thread_key).await {
        Ok(Some(entry)) => entry,
        Ok(None) => return Ok(false),
        Err(error) => {
            return Err(AppError::Internal(format!(
                "Failed to read execution token for thread {}: {}",
                thread_key, error
            )));
        }
    };

    Ok(current.as_ref() != current_execution_token.as_bytes())
}

async fn mark_thread_interrupted(app_state: &AppState, thread_id: i64, deployment_id: i64) {
    let interrupt_cmd = commands::UpdateAgentThreadStateCommand::new(thread_id, deployment_id)
        .with_status(models::AgentThreadStatus::Interrupted);
    let _ = interrupt_cmd
        .execute_with_deps(&common::deps::from_app(app_state).db().nats().id())
        .await;
}
