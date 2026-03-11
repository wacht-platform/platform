use anyhow::Result;
use async_nats::jetstream::{self, AckKind, consumer};
use common::state::AppState;
use dto::json::NatsTaskMessage;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::tasks::{
    agent, analytics, api_audit, api_key_role_permissions_sync, billing, document, email, embedding,
    token, webhook, webhook_event, webhook_replay_batch,
};

const AGENT_EXECUTION_BUSY_RETRY_DELAY_SECONDS: u64 = 30;

#[derive(Debug)]
pub enum TaskError {
    RetryWithDelay(Duration),
    Permanent(String),
}

impl From<anyhow::Error> for TaskError {
    fn from(err: anyhow::Error) -> Self {
        TaskError::Permanent(err.to_string())
    }
}

#[derive(Deserialize)]
struct IncomingTaskMessage {
    task_id: String,
    #[serde(flatten)]
    task: WorkerTask,
}

#[derive(Deserialize)]
#[serde(tag = "task_type", content = "payload")]
enum WorkerTask {
    #[serde(rename = "email.send_verification")]
    EmailSendVerification(email::VerificationEmailTask),
    #[serde(rename = "email.send_password_reset")]
    EmailSendPasswordReset(email::PasswordResetEmailTask),
    #[serde(rename = "email.send_magic_link")]
    EmailSendMagicLink(email::MagicLinkEmailTask),
    #[serde(rename = "email.send_signin_notification")]
    EmailSendSignInNotification(email::SignInNotificationTask),
    #[serde(rename = "email.send_email_change_notification")]
    EmailSendEmailChangeNotification(email::EmailChangeNotificationTask),
    #[serde(rename = "email.send_password_change_notification")]
    EmailSendPasswordChangeNotification(email::PasswordChangeNotificationTask),
    #[serde(rename = "email.send_password_remove_notification")]
    EmailSendPasswordRemoveNotification(email::PasswordRemoveNotificationTask),
    #[serde(rename = "email.send_waitlist_signup")]
    EmailSendWaitlistSignup(email::WaitlistSignupTask),
    #[serde(rename = "email.send_organization_membership_invite")]
    EmailSendOrganizationMembershipInvite(email::OrganizationMembershipInviteTask),
    #[serde(rename = "email.send_deployment_invite")]
    EmailSendDeploymentInvite(email::DeploymentInviteTask),
    #[serde(rename = "email.send_waitlist_approval")]
    EmailSendWaitlistApproval(email::WaitlistApprovalTask),
    #[serde(rename = "token.clean")]
    TokenClean(token::TokenCleanupTask),
    #[serde(rename = "webhook.deliver")]
    WebhookDeliver(webhook::WebhookDeliveryTask),
    #[serde(rename = "webhook.batch")]
    WebhookBatch(webhook::WebhookBatchDeliveryTask),
    #[serde(rename = "webhook.retry")]
    WebhookRetry(webhook::WebhookRetryTask),
    #[serde(rename = "webhook.replay_batch")]
    WebhookReplayBatch(Value),
    #[serde(rename = "document.process")]
    DocumentProcess(document::ProcessDocumentTask),
    #[serde(rename = "agent.execution_request")]
    AgentExecutionRequest(dto::json::AgentExecutionRequest),
    #[serde(rename = "embedding.process_batch")]
    EmbeddingProcessBatch(embedding::ProcessDocumentBatchTask),
    #[serde(rename = "webhook.event")]
    WebhookEvent(webhook_event::WebhookEventTask),
    #[serde(rename = "analytics.event")]
    AnalyticsEvent(analytics::AnalyticsEventTask),
    #[serde(rename = "audit.api_key_verification")]
    ApiAuditEvent(dto::clickhouse::ApiKeyVerificationEvent),
    #[serde(rename = "billing.event")]
    BillingEvent(billing::BillingEventTask),
    #[serde(rename = "api_key.sync_org_membership_permissions")]
    ApiKeySyncOrgMembershipPermissions(dto::json::nats::ApiKeyOrgMembershipSyncPayload),
    #[serde(rename = "api_key.sync_workspace_membership_permissions")]
    ApiKeySyncWorkspaceMembershipPermissions(dto::json::nats::ApiKeyWorkspaceMembershipSyncPayload),
    #[serde(rename = "api_key.sync_org_role_permissions")]
    ApiKeySyncOrgRolePermissions(dto::json::nats::ApiKeyOrgRoleSyncPayload),
    #[serde(rename = "api_key.sync_workspace_role_permissions")]
    ApiKeySyncWorkspaceRolePermissions(dto::json::nats::ApiKeyWorkspaceRoleSyncPayload),
}

impl WorkerTask {
    fn task_type(&self) -> &'static str {
        match self {
            Self::EmailSendVerification(_) => "email.send_verification",
            Self::EmailSendPasswordReset(_) => "email.send_password_reset",
            Self::EmailSendMagicLink(_) => "email.send_magic_link",
            Self::EmailSendSignInNotification(_) => "email.send_signin_notification",
            Self::EmailSendEmailChangeNotification(_) => "email.send_email_change_notification",
            Self::EmailSendPasswordChangeNotification(_) => "email.send_password_change_notification",
            Self::EmailSendPasswordRemoveNotification(_) => "email.send_password_remove_notification",
            Self::EmailSendWaitlistSignup(_) => "email.send_waitlist_signup",
            Self::EmailSendOrganizationMembershipInvite(_) => {
                "email.send_organization_membership_invite"
            }
            Self::EmailSendDeploymentInvite(_) => "email.send_deployment_invite",
            Self::EmailSendWaitlistApproval(_) => "email.send_waitlist_approval",
            Self::TokenClean(_) => "token.clean",
            Self::WebhookDeliver(_) => "webhook.deliver",
            Self::WebhookBatch(_) => "webhook.batch",
            Self::WebhookRetry(_) => "webhook.retry",
            Self::WebhookReplayBatch(_) => "webhook.replay_batch",
            Self::DocumentProcess(_) => "document.process",
            Self::AgentExecutionRequest(_) => "agent.execution_request",
            Self::EmbeddingProcessBatch(_) => "embedding.process_batch",
            Self::WebhookEvent(_) => "webhook.event",
            Self::AnalyticsEvent(_) => "analytics.event",
            Self::ApiAuditEvent(_) => "audit.api_key_verification",
            Self::BillingEvent(_) => "billing.event",
            Self::ApiKeySyncOrgMembershipPermissions(_) => {
                "api_key.sync_org_membership_permissions"
            }
            Self::ApiKeySyncWorkspaceMembershipPermissions(_) => {
                "api_key.sync_workspace_membership_permissions"
            }
            Self::ApiKeySyncOrgRolePermissions(_) => "api_key.sync_org_role_permissions",
            Self::ApiKeySyncWorkspaceRolePermissions(_) => "api_key.sync_workspace_role_permissions",
        }
    }
}

pub struct NatsConsumer {
    jetstream: jetstream::Context,
    app_state: AppState,
}

impl NatsConsumer {
    pub async fn new(app_state: AppState) -> Result<Self> {
        Ok(Self {
            jetstream: app_state.nats_jetstream.clone(),
            app_state,
        })
    }

    async fn execute_task(&self, task_id: &str, task: WorkerTask) -> Result<(), TaskError> {
        match task {
            WorkerTask::EmailSendVerification(task) => {
                email::send_verification_email_impl(
                    task.deployment_id,
                    &task.recipient,
                    &task.verification_code,
                    &task.ip_address,
                    &task.user_agent,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendPasswordReset(task) => {
                email::send_password_reset_email_impl(
                    task.deployment_id,
                    &task.recipient,
                    task.user_id,
                    &task.reset_code,
                    &task.ip_address,
                    &task.user_agent,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendMagicLink(task) => {
                email::send_magic_link_email_impl(
                    task.deployment_id,
                    &task.recipient,
                    task.user_id,
                    &task.magic_link,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendSignInNotification(task) => {
                email::send_signin_notification_email_impl(
                    task.deployment_id,
                    &task.recipient,
                    task.user_id,
                    task.signin_id,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendEmailChangeNotification(task) => {
                email::send_email_change_notification_impl(
                    task.deployment_id,
                    &task.recipient,
                    task.user_id,
                    &task.old_email,
                    &task.new_email,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendPasswordChangeNotification(task) => {
                email::send_password_change_notification_impl(
                    task.deployment_id,
                    &task.recipient,
                    task.user_id,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendPasswordRemoveNotification(task) => {
                email::send_password_remove_notification_impl(
                    task.deployment_id,
                    &task.recipient,
                    task.user_id,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendWaitlistSignup(task) => {
                email::send_waitlist_signup_email_impl(
                    task.deployment_id,
                    &task.recipient,
                    &task.first_name,
                    &task.last_name,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendOrganizationMembershipInvite(task) => {
                email::send_organization_membership_invite_impl(
                    task.deployment_id,
                    &task.recipient,
                    &task.inviter_name,
                    &task.organization_name,
                    &task.invite_link,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendDeploymentInvite(task) => {
                email::send_deployment_invite_impl(
                    task.deployment_id,
                    &task.recipient,
                    task.inviter_user_id,
                    task.deployment_invitation_id,
                    task.workspace_id,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::EmailSendWaitlistApproval(task) => {
                email::send_waitlist_approval_impl(
                    task.deployment_id,
                    &task.recipient,
                    task.deployment_invitation_id,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::TokenClean(task) => {
                token::cleanup_rotating_token_and_session(
                    task.rotating_token_id,
                    task.session_id,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::WebhookDeliver(task) => {
                let result =
                    webhook::process_webhook_delivery(task.delivery_id, task.deployment_id, &self.app_state)
                        .await
                        .map_err(|e| TaskError::Permanent(e.to_string()))?;
                if let webhook::DeliveryResult::RetryAfter(duration) = result {
                    return Err(TaskError::RetryWithDelay(duration));
                }
            }
            WorkerTask::WebhookBatch(task) => {
                webhook::process_webhook_batch(task.delivery_ids, task.deployment_id, &self.app_state)
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::WebhookRetry(task) => {
                webhook::process_webhook_retry(task.delivery_id, task.deployment_id, &self.app_state)
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::WebhookReplayBatch(mut payload) => {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert(
                        "__task_id".to_string(),
                        serde_json::Value::String(task_id.to_string()),
                    );
                }
                webhook_replay_batch::handle_webhook_replay_batch(&self.app_state, payload)
                    .await
                    .map_err(|e| {
                        let msg = e.to_string();
                        if msg.contains("Failed to deserialize webhook replay payload") {
                            TaskError::Permanent(msg)
                        } else {
                            TaskError::RetryWithDelay(Duration::from_secs(30))
                        }
                    })?;
            }
            WorkerTask::DocumentProcess(task) => {
                document::process_document_impl(
                    task.deployment_id,
                    task.knowledge_base_id,
                    task.document_id,
                    &self.app_state,
                )
                .await
                .map_err(|e| {
                    let error_str = e.to_string().to_lowercase();
                    if error_str.contains("query_wait_timeout")
                        || error_str.contains("pool timed out while waiting")
                        || error_str.contains("timeout")
                    {
                        TaskError::RetryWithDelay(Duration::from_secs(10))
                    } else {
                        TaskError::Permanent(e.to_string())
                    }
                })?;
            }
            WorkerTask::AgentExecutionRequest(request) => {
                agent::process_agent_execution(&self.app_state, request)
                    .await
                    .map_err(|e| {
                        match e {
                            agent::AgentExecutionError::ExecutionBusy { .. } => {
                                TaskError::RetryWithDelay(Duration::from_secs(
                                    AGENT_EXECUTION_BUSY_RETRY_DELAY_SECONDS,
                                ))
                            }
                            other => TaskError::Permanent(other.to_string()),
                        }
                    })?;
            }
            WorkerTask::EmbeddingProcessBatch(task) => {
                embedding::process_document_batch_impl(
                    task.deployment_id,
                    task.knowledge_base_id,
                    task.batch_size,
                    &self.app_state,
                )
                .await
                .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::WebhookEvent(task) => {
                webhook_event::trigger_webhook_event(task, &self.app_state).await?;
            }
            WorkerTask::AnalyticsEvent(task) => {
                analytics::store_analytics_event_impl(task, &self.app_state)
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::ApiAuditEvent(task) => {
                api_audit::store_api_audit_event_impl(task, &self.app_state)
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::BillingEvent(task) => {
                billing::process_billing_event(task, &self.app_state)
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))?;
            }
            WorkerTask::ApiKeySyncOrgMembershipPermissions(task) => {
                api_key_role_permissions_sync::sync_org_membership(task, &self.app_state).await?;
            }
            WorkerTask::ApiKeySyncWorkspaceMembershipPermissions(task) => {
                api_key_role_permissions_sync::sync_workspace_membership(task, &self.app_state)
                    .await?;
            }
            WorkerTask::ApiKeySyncOrgRolePermissions(task) => {
                api_key_role_permissions_sync::sync_org_role(task, &self.app_state).await?;
            }
            WorkerTask::ApiKeySyncWorkspaceRolePermissions(task) => {
                api_key_role_permissions_sync::sync_workspace_role(task, &self.app_state).await?;
            }
        }

        Ok(())
    }

    pub async fn start_consuming(&self) -> Result<()> {
        info!("Starting NATS JetStream consumer for worker tasks");

        let stream = self.jetstream.get_stream("worker_tasks").await?;

        let consumer = match stream
            .create_consumer(consumer::pull::Config {
                durable_name: Some("worker-processor".to_string()),
                filter_subject: "worker.tasks.>".to_string(),
                ..Default::default()
            })
            .await
        {
            Ok(consumer) => consumer,
            Err(_) => stream
                .get_consumer("worker-processor")
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get consumer: {}", e))?,
        };

        let messages = consumer.messages().await?.take(5000);

        messages
            .for_each_concurrent(1000, async |message| {
                if message.is_err() {
                    error!("Error getting message: {}", message.err().unwrap());
                    return;
                }
                let message = message.unwrap();
                if let Err(e) = self.handle_message(message).await {
                    error!("Error handling JetStream message: {}", e);
                }
            })
            .await;

        Ok(())
    }

    async fn handle_message(&self, message: async_nats::jetstream::Message) -> Result<()> {
        let raw_payload = String::from_utf8_lossy(&message.payload);

        let task_message: IncomingTaskMessage = match serde_json::from_slice(&message.payload) {
            Ok(msg) => msg,
            Err(e) => {
                if let Ok(raw_msg) = serde_json::from_slice::<NatsTaskMessage>(&message.payload) {
                    warn!(
                        "Unknown or invalid task type '{}' (ID: {})",
                        raw_msg.task_type, raw_msg.task_id
                    );
                } else {
                    error!(
                        "Failed to deserialize worker task message: {}. Raw payload (first 500 chars): {}",
                        e,
                        raw_payload.chars().take(500).collect::<String>()
                    );
                }
                if let Err(ack_err) = message.ack().await {
                    error!("Failed to ack malformed message: {}", ack_err);
                }
                return Err(anyhow::anyhow!("Deserialization failed: {}", e));
            }
        };

        info!(
            "Received JetStream task: {} (ID: {})",
            task_message.task.task_type(),
            task_message.task_id
        );

        match self
            .execute_task(&task_message.task_id, task_message.task)
            .await
        {
            Ok(_) => {
                info!("Task {} completed successfully", task_message.task_id);
                if let Err(e) = message.ack().await {
                    error!("Failed to acknowledge task {}: {}", task_message.task_id, e);
                }
            }
            Err(TaskError::RetryWithDelay(duration)) => {
                info!(
                    "Task {} will retry after {:?}",
                    task_message.task_id, duration
                );
                if let Err(e) = message.ack_with(AckKind::Nak(Some(duration))).await {
                    error!(
                        "Failed to NAK with delay for task {}: {}",
                        task_message.task_id, e
                    );
                }
            }
            Err(TaskError::Permanent(error_msg)) => {
                error!("Task {} permanently failed: {}", task_message.task_id, error_msg);
                if let Err(e) = message.ack().await {
                    error!(
                        "Failed to acknowledge failed task {}: {}",
                        task_message.task_id, e
                    );
                }
            }
        }

        Ok(())
    }
}
