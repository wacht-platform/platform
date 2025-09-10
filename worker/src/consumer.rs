use anyhow::Result;
use async_nats::jetstream::{self, AckKind, consumer};
use common::state::AppState;
use futures::StreamExt;
use serde_json;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::tasks::{agent, document, email, embedding, sms, token, webhook, webhook_replay_batch};
use dto::json::NatsTaskMessage;

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

pub struct NatsConsumer {
    jetstream: jetstream::Context,
    task_handlers: HashMap<String, TaskHandler>,
    app_state: AppState,
}

type TaskHandler = Box<
    dyn Fn(
            serde_json::Value,
            AppState,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, TaskError>> + Send>>
        + Send
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
                        .map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_verification_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &task.verification_code,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_password_reset".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::PasswordResetEmailTask = serde_json::from_value(payload)
                        .map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_password_reset_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &task.reset_code,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_magic_link".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::MagicLinkEmailTask =
                        serde_json::from_value(payload).map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_magic_link_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &task.magic_link,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_signin_notification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::SignInNotificationTask = serde_json::from_value(payload)
                        .map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_signin_notification_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        task.signin_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_email_change_notification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::EmailChangeNotificationTask = serde_json::from_value(payload)
                        .map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_email_change_notification_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &task.old_email,
                        &task.new_email,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_password_change_notification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::PasswordChangeNotificationTask =
                        serde_json::from_value(payload).map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_password_change_notification_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_password_remove_notification".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::PasswordRemoveNotificationTask =
                        serde_json::from_value(payload).map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_password_remove_notification_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_waitlist_signup".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::WaitlistSignupTask =
                        serde_json::from_value(payload).map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_waitlist_signup_email_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.user_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_organization_membership_invite".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::OrganizationMembershipInviteTask =
                        serde_json::from_value(payload).map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_organization_membership_invite_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.inviter_user_id,
                        task.organization_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_deployment_invite".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::DeploymentInviteTask = serde_json::from_value(payload)
                        .map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_deployment_invite_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.inviter_user_id,
                        task.deployment_invitation_id,
                        task.workspace_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "email.send_waitlist_approval".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: email::WaitlistApprovalTask = serde_json::from_value(payload)
                        .map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    email::send_waitlist_approval_impl(
                        task.deployment_id,
                        &task.recipient,
                        task.deployment_invitation_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "sms.send_otp".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: sms::SMSOTPTask = serde_json::from_value(payload).map_err(|e| {
                        TaskError::Permanent(format!("Failed to deserialize SMS OTP task: {}", e))
                    })?;
                    sms::send_otp_sms(
                        task.deployment_id,
                        &task.phone_number,
                        &task.country_code,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "token.clean".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: token::TokenCleanupTask =
                        serde_json::from_value(payload).map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;
                    token::cleanup_rotating_token_and_session(
                        task.rotating_token_id,
                        task.session_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "webhook.deliver".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: webhook::WebhookDeliveryTask = serde_json::from_value(payload)
                        .map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize task: {}", e))
                        })?;

                    let result = webhook::process_webhook_delivery(
                        task.delivery_id,
                        task.deployment_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))?;

                    // Check if we need to retry with delay
                    match result {
                        webhook::DeliveryResult::RetryAfter(duration) => {
                            Err(TaskError::RetryWithDelay(duration))
                        }
                        _ => Ok(format!("{:?}", result)),
                    }
                })
            }),
        );

        task_handlers.insert(
            "webhook.batch".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: webhook::WebhookBatchDeliveryTask = serde_json::from_value(payload)
                        .map_err(|e| {
                        TaskError::Permanent(format!("Failed to deserialize batch task: {}", e))
                    })?;
                    webhook::process_webhook_batch(
                        task.delivery_ids,
                        task.deployment_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "webhook.retry".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: webhook::WebhookRetryTask =
                        serde_json::from_value(payload).map_err(|e| {
                            TaskError::Permanent(format!("Failed to deserialize retry task: {}", e))
                        })?;
                    webhook::process_webhook_retry(task.delivery_id, task.deployment_id, &app_state)
                        .await
                        .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "webhook.replay_batch".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    webhook_replay_batch::handle_webhook_replay_batch(&app_state, payload)
                        .await
                        .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "document.process".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: document::ProcessDocumentTask = serde_json::from_value(payload)
                        .map_err(|e| {
                            TaskError::Permanent(format!(
                                "Failed to deserialize document processing task: {}",
                                e
                            ))
                        })?;
                    document::process_document_impl(
                        task.deployment_id,
                        task.knowledge_base_id,
                        task.document_id,
                        &app_state,
                    )
                    .await
                    .map_err(|e| {
                        let error_str = e.to_string().to_lowercase();
                        // Retry on query timeouts and pool exhaustion
                        if error_str.contains("query_wait_timeout") 
                            || error_str.contains("pool timed out while waiting")
                            || error_str.contains("timeout") {
                            TaskError::RetryWithDelay(Duration::from_secs(10))
                        } else {
                            TaskError::Permanent(e.to_string())
                        }
                    })
                })
            }),
        );

        task_handlers.insert(
            "agent.stream_log".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: agent::AgentStreamLogTask =
                        serde_json::from_value(payload).map_err(|e| {
                            TaskError::Permanent(format!(
                                "Failed to deserialize agent stream log task: {}",
                                e
                            ))
                        })?;
                    agent::log_agent_stream_message(&app_state, task)
                        .await
                        .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

        task_handlers.insert(
            "embedding.process_batch".to_string(),
            Box::new(|payload, app_state| {
                Box::pin(async move {
                    let task: embedding::ProcessDocumentBatchTask = serde_json::from_value(payload)
                        .map_err(|e| {
                            TaskError::Permanent(format!(
                                "Failed to deserialize embedding batch task: {}",
                                e
                            ))
                        })?;
                    embedding::process_document_batch_impl(
                        task.deployment_id,
                        task.knowledge_base_id,
                        task.batch_size,
                        &app_state,
                    )
                    .await
                    .map_err(|e| TaskError::Permanent(e.to_string()))
                })
            }),
        );

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
        let task_message: NatsTaskMessage = serde_json::from_slice(&message.payload)?;

        info!(
            "Received JetStream task: {} (ID: {})",
            task_message.task_type, task_message.task_id
        );

        if let Some(handler) = self.task_handlers.get(&task_message.task_type) {
            match handler(task_message.payload, self.app_state.clone()).await {
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
                    error!(
                        "Task {} permanently failed: {}",
                        task_message.task_id, error_msg
                    );
                    // ACK permanent failures to remove from queue
                    if let Err(e) = message.ack().await {
                        error!(
                            "Failed to acknowledge failed task {}: {}",
                            task_message.task_id, e
                        );
                    }
                }
            }
        } else {
            warn!("Unknown task type: {}", task_message.task_type);
            if let Err(e) = message.ack().await {
                error!(
                    "Failed to acknowledge unknown task {}: {}",
                    task_message.task_id, e
                );
            }
        }

        Ok(())
    }
}
