use anyhow::Result;
use async_nats::jetstream::{self, consumer};
use futures::StreamExt;
use serde_json;
use shared::state::AppState;
use std::collections::HashMap;
use tracing::{error, info, warn};

use crate::nats_types::{NatsTaskMessage, TaskResult};
use crate::tasks::{email, sms, token, webhook};

pub struct NatsConsumer {
    jetstream: jetstream::Context,
    task_handlers: HashMap<String, TaskHandler>,
    app_state: AppState,
}

type TaskHandler = Box<
    dyn Fn(
            serde_json::Value,
            AppState,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, anyhow::Error>> + Send>,
        > + Send
        + Sync,
>;

impl NatsConsumer {
    pub async fn new(app_state: AppState) -> Result<Self> {
        let mut task_handlers: HashMap<String, TaskHandler> = HashMap::new();

        task_handlers.insert(
            "email.send_verification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::VerificationEmailTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_verification_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &task.verification_code,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_password_reset".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::PasswordResetEmailTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_password_reset_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &task.reset_code,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_magic_link".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::MagicLinkEmailTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_magic_link_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &task.magic_link,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_signin_notification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::SignInNotificationTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_signin_notification_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        task.signin_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_email_change_notification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::EmailChangeNotificationTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_email_change_notification_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &task.old_email,
                        &task.new_email,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_password_change_notification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::PasswordChangeNotificationTask =
                        serde_json::from_value(payload)
                            .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_password_change_notification_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_password_remove_notification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::PasswordRemoveNotificationTask =
                        serde_json::from_value(payload)
                            .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_password_remove_notification_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_waitlist_signup".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::WaitlistSignupTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_waitlist_signup_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_organization_membership_invite".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::OrganizationMembershipInviteTask =
                        serde_json::from_value(payload)
                            .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_organization_membership_invite_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.inviter_user_id,
                        task.organization_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_deployment_invite".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::DeploymentInviteTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_deployment_invite_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.inviter_user_id,
                        task.deployment_invitation_id,
                        task.workspace_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "email.send_waitlist_approval".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::WaitlistApprovalTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    email::send_waitlist_approval_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.deployment_invitation_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "sms.send".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: sms::SMSTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    sms::send_sms_by_type(&task.task_type, task.deployment_id, &task.phone_number, &app_state)
                        .await
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "token.clean".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: token::TokenCleanupTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    token::cleanup_rotating_token_and_session(
                        task.rotating_token_id,
                        task.session_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "webhook.deliver".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: webhook::WebhookDeliveryTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize task: {}", e))?;
                    webhook::process_webhook_delivery(
                        task.delivery_id,
                        task.deployment_id,
                        &app_state,
                    )
                    .await
                    .map(|result| format!("{:?}", result))
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        task_handlers.insert(
            "webhook.batch".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: webhook::WebhookBatchDeliveryTask = serde_json::from_value(payload)
                        .map_err(|e| anyhow::anyhow!("Failed to deserialize batch task: {}", e))?;
                    webhook::process_webhook_batch(
                        task.delivery_ids,
                        task.deployment_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );

        // Cleanup should be handled by a cron job, not through NATS

        Ok(Self {
            jetstream: app_state.nats_jetstream.clone(),
            task_handlers,
            app_state,
        })
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
            .for_each_concurrent(5000, async |message| {
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
        let task_message: NatsTaskMessage = serde_json::from_slice(&message.payload)?;

        info!(
            "Received JetStream task: {} (ID: {})",
            task_message.task_type, task_message.task_id
        );

        let result = if let Some(handler) = self.task_handlers.get(&task_message.task_type) {
            match handler(task_message.payload, self.app_state.clone()).await {
                Ok(result) => {
                    info!("Task {} completed successfully", task_message.task_id);
                    TaskResult::success(task_message.task_id.clone(), result)
                }
                Err(e) => {
                    error!("Task {} failed: {}", task_message.task_id, e);
                    TaskResult::error(task_message.task_id.clone(), e.to_string())
                }
            }
        } else {
            warn!("Unknown task type: {}", task_message.task_type);
            TaskResult::error(
                task_message.task_id.clone(),
                format!("Unknown task type: {}", task_message.task_type),
            )
        };

        if result.success {
            if let Err(e) = message.ack().await {
                error!(
                    "Failed to acknowledge successful task {}: {}",
                    task_message.task_id, e
                );
            } else {
                info!("Task {} acknowledged successfully", task_message.task_id);
            }
        }

        Ok(())
    }
}
