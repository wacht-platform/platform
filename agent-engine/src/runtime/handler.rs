use common::ResultExt;
use std::sync::Arc;

use crate::executor::core::AgentExecutorBuilder;
use crate::runtime::secrets_provider::{SecretsProvider, SettingsSecretsProvider};
use crate::runtime::vector_store::{LanceDbVectorStoreFactory, VectorStoreFactory};
use crate::sandbox::self_healing::SelfHealingHandle;
use crate::sandbox::{
    SandboxHandle, SandboxMount, SandboxMountMode, SandboxRuntimeFactory, TaskSandboxSpec,
    ThreadSandboxSpec,
};
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
    sandbox_factory: Arc<SandboxRuntimeFactory>,
    secrets_provider: Arc<dyn SecretsProvider>,
    vector_store_factory: Arc<dyn VectorStoreFactory>,
}

enum ExecutionMode {
    ApprovalResponse(Vec<dto::json::deployment::ToolApprovalSelection>),
    Conversation(i64),
    ThreadEvent(ThreadEvent),
}

struct TaskSandboxRef {
    task_key: String,
    mounts: Vec<SandboxMount>,
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
    pub event_log_id: Option<i64>,
    pub execution_run_id: i64,
    pub execution_token: String,
    pub watch_key: String,
    pub approval_response: Option<Vec<dto::json::deployment::ToolApprovalSelection>>,
    pub thread_event: Option<ThreadEvent>,
}

impl AgentHandler {
    pub fn new(app_state: AppState) -> Self {
        let secrets_provider: Arc<dyn SecretsProvider> = Arc::new(SettingsSecretsProvider::new(
            app_state.encryption_service.clone(),
        ));
        let vector_store_factory: Arc<dyn VectorStoreFactory> =
            Arc::new(LanceDbVectorStoreFactory::new(app_state.clone()));
        let sandbox_factory = build_sandbox_factory(&app_state);
        Self {
            app_state,
            sandbox_factory,
            secrets_provider,
            vector_store_factory,
        }
    }

    pub fn with_secrets_provider(mut self, secrets_provider: Arc<dyn SecretsProvider>) -> Self {
        self.secrets_provider = secrets_provider;
        self
    }

    pub fn with_vector_store_factory(
        mut self,
        vector_store_factory: Arc<dyn VectorStoreFactory>,
    ) -> Self {
        self.vector_store_factory = vector_store_factory;
        self
    }

    async fn resolve_sandbox_handle(
        &self,
        request: &ExecutionRequest,
        thread_state: &models::AgentThreadState,
        task_sandbox: Option<TaskSandboxRef>,
    ) -> Result<Arc<dyn SandboxHandle>, AppError> {
        let deployment_id = request.agent.deployment_id;
        let runtime = self
            .sandbox_factory
            .for_deployment(&deployment_id.to_string());

        if let Some(task_ref) = task_sandbox {
            let spec = TaskSandboxSpec {
                deployment_id: deployment_id.to_string(),
                project_id: thread_state.project_id.to_string(),
                task_key: task_ref.task_key,
                mounts: task_ref.mounts,
            };
            let initial = runtime
                .ensure_task_sandbox(spec.clone())
                .await
                .map_err_internal("ensure task sandbox")?;
            let initial_arc: Arc<dyn SandboxHandle> = Arc::from(initial);
            let label = format!("task[{}/{}]", spec.project_id, spec.task_key);
            let recreate_runtime = runtime.clone();
            let recreate_spec = spec.clone();
            let recreate = Arc::new(move || -> crate::sandbox::self_healing::RecreateFuture {
                let runtime = recreate_runtime.clone();
                let spec = recreate_spec.clone();
                Box::pin(async move {
                    let fresh = runtime.ensure_task_sandbox(spec).await?;
                    let arc: Arc<dyn SandboxHandle> = Arc::from(fresh);
                    Ok(arc)
                })
            });
            return Ok(Arc::new(SelfHealingHandle::new(
                initial_arc,
                recreate,
                label,
            )));
        }

        let spec = ThreadSandboxSpec {
            deployment_id: deployment_id.to_string(),
            thread_id: request.thread_id.to_string(),
            project_id: Some(thread_state.project_id.to_string()),
            agent_id: Some(request.agent.id.to_string()),
        };
        let initial = runtime
            .ensure_thread_sandbox(spec.clone())
            .await
            .map_err_internal("ensure thread sandbox")?;
        let initial_arc: Arc<dyn SandboxHandle> = Arc::from(initial);
        let label = format!("thread[{}]", spec.thread_id);
        let recreate_runtime = runtime.clone();
        let recreate_spec = spec.clone();
        let recreate = Arc::new(move || -> crate::sandbox::self_healing::RecreateFuture {
            let runtime = recreate_runtime.clone();
            let spec = recreate_spec.clone();
            Box::pin(async move {
                let fresh = runtime.ensure_thread_sandbox(spec).await?;
                let arc: Arc<dyn SandboxHandle> = Arc::from(fresh);
                Ok(arc)
            })
        });
        Ok(Arc::new(SelfHealingHandle::new(
            initial_arc,
            recreate,
            label,
        )))
    }

    async fn resolve_task_sandbox_ref(
        &self,
        request: &ExecutionRequest,
    ) -> Result<Option<TaskSandboxRef>, AppError> {
        let Some(event) = request.thread_event.as_ref() else {
            return Ok(None);
        };
        if !matches!(
            event.event_type.as_str(),
            models::thread_event::event_type::TASK_ROUTING
                | models::thread_event::event_type::ASSIGNMENT_EXECUTION
        ) {
            return Ok(None);
        }
        let Some(board_item_id) = event.board_item_id else {
            return Ok(None);
        };
        let Some(item) = queries::GetProjectTaskBoardItemByIdQuery::new(board_item_id)
            .execute_with_db(self.app_state.db_router.writer())
            .await?
        else {
            return Ok(None);
        };
        let mounts = models::project_task_schedule::parse_mounts(&item.mounts)
            .map_err_internal("Invalid task mounts")?
            .into_iter()
            .map(|m| SandboxMount {
                mount_path: m.mount_path,
                s3_relative_key: m.s3_relative_key,
                mode: if m.mode == models::project_task_schedule::mount_mode::RO {
                    SandboxMountMode::Ro
                } else {
                    SandboxMountMode::Rw
                },
            })
            .collect();
        Ok(Some(TaskSandboxRef {
            task_key: item.task_key,
            mounts,
        }))
    }

    #[tracing::instrument(
        name = "agent_handler.execute_streaming",
        skip(self, request),
        fields(
            deployment_id = request.agent.deployment_id,
            agent_id = request.agent.id,
            thread_id = request.thread_id,
            execution_run_id = request.execution_run_id,
            conversation_id = ?request.conversation_id,
            event_log_id = ?request.event_log_id,
            has_approvals = request.approval_response.is_some(),
            has_thread_event = request.thread_event.is_some(),
        ),
    )]
    pub async fn execute_agent_streaming(&self, request: ExecutionRequest) -> Result<(), AppError> {
        let (sender, receiver) = tokio::sync::mpsc::channel::<StreamEvent>(100);
        let thread_key = request.thread_id.to_string();
        let deployment_id = request.agent.deployment_id;

        let db = self.app_state.db_router.writer();
        let settings_query = queries::GetDeploymentAiSettingsQuery::new(deployment_id);
        let thread_query = queries::GetAgentThreadStateQuery::new(request.thread_id, deployment_id);
        let profiles_query = queries::ListDeploymentAiProviderProfilesQuery::new(deployment_id);
        let settings_fut = settings_query.execute_with_db(db);
        let thread_fut = thread_query.execute_with_db(db);
        let profiles_fut = profiles_query.execute_with_db(db);
        let task_ref_fut = self.resolve_task_sandbox_ref(&request);
        let (settings_result, thread_result, profiles_result, task_ref_result) =
            tokio::join!(settings_fut, thread_fut, profiles_fut, task_ref_fut);
        let deployment_ai_settings = settings_result.ok().flatten();
        let thread_state = thread_result?;
        let provider_profiles = profiles_result?;
        let task_sandbox_ref = task_ref_result?;

        let provider_keys = self
            .secrets_provider
            .resolve_provider_keys(deployment_ai_settings.as_ref(), &provider_profiles)
            .await?;

        let vector_store = self
            .vector_store_factory
            .create(deployment_id, provider_keys.embedding_dimension);

        let execution_context =
            crate::runtime::thread_execution_context::ThreadExecutionContext::new_with_thread(
                self.app_state.clone(),
                request.agent.clone(),
                request.thread_id,
                thread_state.actor_id,
                request.execution_run_id,
                provider_keys,
                vector_store,
                Some(thread_state.clone()),
            );

        self.spawn_message_publisher(receiver, thread_key.clone(), execution_context.clone());

        let execution_context_for_notification = execution_context.clone();

        let app_state_for_mark = self.app_state.clone();
        let thread_id_for_mark = thread_state.id;
        let mark_running_fut = async move {
            commands::UpdateAgentThreadStateCommand::new(thread_id_for_mark, deployment_id)
                .with_status(AgentThreadStatus::Running)
                .execute_with_deps(&common::deps::from_app(&app_state_for_mark).db().nats().id())
                .await
        };
        let board_item_id = request
            .thread_event
            .as_ref()
            .and_then(|event| event.board_item_id);
        let (sandbox_handle, prepared, _mark) = tokio::try_join!(
            self.resolve_sandbox_handle(&request, &thread_state, task_sandbox_ref),
            AgentExecutorBuilder::prepare(execution_context.clone(), board_item_id),
            mark_running_fut,
        )?;
        let mut executor = AgentExecutorBuilder::finalize(prepared, sandbox_handle, sender)?;

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
        let defer_guard = if thread_state.thread_purpose
            == models::agent_thread::purpose::COORDINATOR
        {
            Some(commands::event_log::DeferredDispatch::new())
        } else {
            None
        };

        let execution_future = self.run_execution_mode(
            &request.watch_key,
            &request.execution_token,
            &mut executor,
            execution_mode,
            thread_state.id,
            deployment_id,
        );

        let execution_result = match defer_guard {
            Some(guard) => {
                commands::event_log::run_with_deferred_dispatch(
                    &self.app_state.nats_client,
                    guard,
                    execution_future,
                )
                .await
            }
            None => execution_future.await,
        };

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
            let _ = clear_execution_token_if_current(
                &request.watch_key,
                &request.execution_token,
                &self.app_state,
            )
            .await;
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
        watch_key: &str,
        execution_token: &str,
        agent_executor: &mut AgentExecutor,
        execution_mode: ExecutionMode,
        thread_id: i64,
        deployment_id: i64,
    ) -> Result<(), AppError> {
        match execution_mode {
            ExecutionMode::ApprovalResponse(approvals) => {
                self.run_with_execution_watch(
                    watch_key,
                    execution_token,
                    thread_id,
                    deployment_id,
                    agent_executor.resume_execution(ResumeContext::ApprovalResponse(approvals)),
                )
                .await
            }
            ExecutionMode::Conversation(conversation_id) => {
                self.run_with_execution_watch(
                    watch_key,
                    execution_token,
                    thread_id,
                    deployment_id,
                    agent_executor.execute_with_conversation_id(conversation_id),
                )
                .await
            }
            ExecutionMode::ThreadEvent(thread_event) => {
                if let Some(assignment_id) = assignment_id_from_thread_event(&thread_event) {
                    let assignment =
                        queries::GetProjectTaskBoardItemAssignmentByIdQuery::new(assignment_id)
                            .execute_with_db(self.app_state.db_router.writer())
                            .await?;
                    if let Some(a) = assignment.as_ref() {
                        if matches!(a.status.as_str(), "cancelled" | "completed" | "rejected") {
                            tracing::info!(
                                thread_id,
                                deployment_id,
                                assignment_id,
                                board_item_id = ?thread_event.board_item_id,
                                status = %a.status,
                                "Skipping ASSIGNMENT_EXECUTION: assignment is no longer active"
                            );
                            return Ok(());
                        }
                    }
                    if let Some(board_item_id) = thread_event.board_item_id {
                        if let Some(item) =
                            queries::GetProjectTaskBoardItemByIdQuery::new(board_item_id)
                                .execute_with_db(self.app_state.db_router.writer())
                                .await?
                        {
                            if matches!(item.status.as_str(), "cancelled" | "completed") {
                                tracing::info!(
                                    thread_id,
                                    deployment_id,
                                    assignment_id,
                                    board_item_id,
                                    status = %item.status,
                                    "Skipping ASSIGNMENT_EXECUTION: board item is no longer active"
                                );
                                return Ok(());
                            }
                        }
                    }

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
                    watch_key,
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
        watch_key: &str,
        execution_token: &str,
        thread_id: i64,
        deployment_id: i64,
        execution_future: F,
    ) -> Result<(), AppError>
    where
        F: std::future::Future<Output = Result<(), AppError>>,
    {
        if execution_token.is_empty() {
            return execution_future.await;
        }
        tokio::pin!(execution_future);
        let mut token_watch =
            watch_execution_token_changes(&self.app_state, watch_key, execution_token).await?;

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
                                "Failed to watch execution token for {}: {}",
                                watch_key, error
                            )));
                        }
                        None => {
                            if execution_token_superseded(&self.app_state, watch_key, execution_token).await? {
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
        self.finalize_event_log_work(request, execution_result)
            .await;
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

    async fn finalize_event_log_work(
        &self,
        request: &ExecutionRequest,
        execution_result: &Result<(), AppError>,
    ) {
        let Some(event_log_id) = request.event_log_id else {
            return;
        };
        if execution_result.is_ok() {
            let _ = commands::event_log::mark_event_work_completed(
                self.app_state.db_router.writer(),
                event_log_id,
                chrono::Utc::now(),
            )
            .await;
        } else {
            let _ = commands::event_log::mark_event_failed(
                self.app_state.db_router.writer(),
                event_log_id,
                "execution failed",
            )
            .await;
        }
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

    let (message_type, payload) =
        dto::json::encode_stream_event(&event).map_err_internal("Failed to encode stream event")?;

    let mut headers = async_nats::HeaderMap::new();
    headers.insert("message_type", message_type.as_header_value());
    headers.insert("thread_id", thread_key);
    headers.insert("deployment_id", deployment_id.to_string().as_str());

    jetstream
        .publish_with_headers(subject.clone(), headers, payload.clone().into())
        .await
        .map_err_internal("Failed to publish to NATS")?;

    use commands::webhook_trigger::TriggerWebhookEventCommand;

    let webhook_payload = serde_json::json!({
        "thread_id": thread_key,
        "message_type": message_type.as_header_value(),
        "data": serde_json::from_slice::<serde_json::Value>(&payload).unwrap_or(serde_json::Value::Null),
        "timestamp": chrono::Utc::now(),
    });

    let console_id = crate::console_deployment_id();

    let trigger_command = TriggerWebhookEventCommand::new(
        console_id,
        deployment_id.to_string(),
        message_type.webhook_event_name().to_string(),
        webhook_payload,
    );

    let _ = trigger_command
        .execute_with_deps(&common::deps::from_app(app_state).db().redis().nats().id())
        .await;

    Ok(())
}

async fn watch_execution_token_changes(
    app_state: &AppState,
    watch_key: &str,
    current_execution_token: &str,
) -> Result<async_nats::jetstream::kv::Watch, AppError> {
    let kv = app_state
        .nats_jetstream
        .get_key_value("agent_execution_kv")
        .await
        .map_err(|error| {
            AppError::Internal(format!(
                "Failed to access execution token KV bucket for {}: {}",
                watch_key, error
            ))
        })?;

    let mut watch = kv.watch_with_history(watch_key).await.map_err(|error| {
        AppError::Internal(format!(
            "Failed to create execution token watch for {}: {}",
            watch_key, error
        ))
    })?;

    while let Some(entry) = watch.next().await {
        let entry = entry.map_err(|error| {
            AppError::Internal(format!(
                "Failed to initialize execution token watch for {}: {}",
                watch_key, error
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
    watch_key: &str,
    current_execution_token: &str,
    app_state: &AppState,
) -> Result<(), async_nats::Error> {
    let kv = app_state
        .nats_jetstream
        .get_key_value("agent_execution_kv")
        .await?;
    match kv.get(watch_key).await? {
        Some(entry) if entry.as_ref() == current_execution_token.as_bytes() => {
            kv.delete(watch_key).await?;
        }
        _ => {}
    }
    Ok(())
}

async fn execution_token_superseded(
    app_state: &AppState,
    watch_key: &str,
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

    let current = match kv.get(watch_key).await {
        Ok(Some(entry)) => entry,
        Ok(None) => return Ok(false),
        Err(error) => {
            return Err(AppError::Internal(format!(
                "Failed to read execution token for {}: {}",
                watch_key, error
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

fn build_sandbox_factory(_app_state: &AppState) -> Arc<SandboxRuntimeFactory> {
    crate::sandbox::shared_sandbox_runtime()
}
