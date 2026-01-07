use super::core::AgentExecutor;

use commands::{Command, CreateConversationCommand, UpdateExecutionContextQuery};
use common::error::AppError;
use dto::json::StreamEvent;
use models::{
    AgentExecutionState, ConversationContent, ConversationMessageType, ConversationRecord,
    ExecutionContextStatus, UserInputRequestState,
};
use serde_json::{json, Value};
use std::collections::HashMap;

impl AgentExecutor {
    pub(super) async fn store_conversation(
        &mut self,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Result<(), AppError> {
        let conversation = self.create_conversation(content, message_type).await?;
        self.conversations.push(conversation.clone());

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation))
            .await;

        Ok(())
    }

    pub(super) async fn create_conversation(
        &self,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Result<ConversationRecord, AppError> {
        let command = CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            content,
            message_type,
        );
        command.execute(&self.app_state).await
    }

    pub(super) async fn store_user_message(
        &self,
        message: String,
        images: Option<Vec<dto::json::agent_executor::ImageData>>,
    ) -> Result<ConversationRecord, AppError> {
        let model_images = if let Some(imgs) = images {
            let mut uploaded_images = Vec::new();

            for img in imgs {
                use base64::{engine::general_purpose::STANDARD, Engine};
                let bytes = STANDARD.decode(&img.data).map_err(|e| {
                    AppError::BadRequest(format!("Invalid base64 image data: {}", e))
                })?;

                let file_extension = img.mime_type.split('/').last().unwrap_or("png");
                let filename = format!(
                    "{}.{}",
                    self.app_state.sf.next_id()?,
                    file_extension
                );

                let relative_path = self.filesystem.save_upload(&filename, &bytes).await?;

                uploaded_images.push(models::ImageData {
                    mime_type: img.mime_type,
                    url: relative_path,
                    size_bytes: Some(img.data.len() as u64),
                });
            }

            Some(uploaded_images)
        } else {
            None
        };

        let command = CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            ConversationContent::UserMessage {
                message,
                sender_name: None,
                images: model_images,
            },
            ConversationMessageType::UserMessage,
        );
        let conversation = command.execute(&self.app_state).await?;

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation.clone()))
            .await;

        Ok(conversation)
    }

    pub(super) async fn get_conversation_history_for_llm(&self) -> Vec<Value> {
        let mut history = Vec::new();
        let mut i = 0;

        while i < self.conversations.len() {
            let conv = &self.conversations[i];

            match conv.message_type {
                ConversationMessageType::ExecutionSummary => {
                    if let ConversationContent::ExecutionSummary {
                        user_message,
                        agent_execution,
                        ..
                    } = &conv.content
                    {
                        history.push(json!({
                            "role": "user",
                            "content": user_message,
                            "timestamp": conv.created_at,
                            "type": "user_message",
                        }));

                        history.push(json!({
                            "role": "model",
                            "content": agent_execution,
                            "timestamp": conv.created_at,
                            "type": "execution_summary",
                        }));

                        i += 2;
                    }
                }
                ConversationMessageType::UserMessage => {
                    if let ConversationContent::UserMessage { message, images, .. } = &conv.content {
                        let mut parts = vec![json!({
                            "text": message
                        })];

                        if let Some(imgs) = images {
                            for img in imgs {
                                use base64::{engine::general_purpose::STANDARD, Engine};
                                
                                if let Ok(bytes) = self.filesystem.read_file_bytes(&img.url).await {
                                    let base64_data = STANDARD.encode(&bytes);
                                    parts.push(json!({
                                        "inline_data": {
                                            "mime_type": img.mime_type,
                                            "data": base64_data
                                        }
                                    }));
                                }
                            }
                        }

                        history.push(json!({
                            "role": "user",
                            "parts": parts,
                            "timestamp": conv.created_at,
                            "type": conv.message_type,
                        }));
                    } else {
                        history.push(json!({
                            "role": "user",
                            "content": self.extract_conversation_content(&conv.content),
                            "timestamp": conv.created_at,
                            "type": conv.message_type,
                        }));
                    }
                    i += 1;
                }
                _ => {
                    history.push(json!({
                        "role": self.map_conversation_type_to_role(&conv.message_type),
                        "content": self.extract_conversation_content(&conv.content),
                        "timestamp": conv.created_at,
                        "type": conv.message_type,
                    }));
                    i += 1;
                }
            }
        }



        // DEBUG: Print formatted history
        println!("\n=== LLM CONVERSATION HISTORY ===");
        for item in &history {
            let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("unknown");
            let content = if let Some(parts) = item.get("parts") {
                format!("{:?}", parts)
            } else {
                item.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string()
            };
            println!("[{}] {}", role.to_uppercase(), content);
        }
        println!("================================\n");

        history
    }

    pub(super) fn map_conversation_type_to_role(&self, msg_type: &ConversationMessageType) -> &'static str {
        match msg_type {
            ConversationMessageType::UserMessage => "user",
            _ => "model",
        }
    }

    pub(super) fn get_working_memory(&self) -> HashMap<String, Value> {
        let mut memory = HashMap::new();

        memory.insert("user_request".to_string(), json!(self.user_request));

        memory.insert(
            "current_iteration".to_string(),
            json!(self.conversations.len()),
        );

        if !self.task_results.is_empty() {
            let successful_tasks = self
                .task_results
                .values()
                .filter(|r| r.status == "completed")
                .count();
            memory.insert("successful_task_count".to_string(), json!(successful_tasks));
        }

        memory
    }

    pub(super) fn extract_conversation_content(&self, content: &ConversationContent) -> String {
        match content {
            ConversationContent::UserMessage { message, .. } => message.clone(),
            ConversationContent::AssistantAcknowledgment {
                acknowledgment_message,
                ..
            } => acknowledgment_message.clone(),
            ConversationContent::AgentResponse { response, .. } => response.clone(),
            ConversationContent::UserInputRequest { question, .. } => question.clone(),
            ConversationContent::SystemDecision { step, reasoning, .. } => {
                format!("System Decision (Step: {}): {}", step, reasoning)
            },
            _ => serde_json::to_string(content).unwrap_or_default(),
        }
    }

    pub fn post_execution_processing(mut self) {
        tokio::spawn(async move { if let Err(_e) = self.check_and_generate_summaries().await {} });
    }

    pub(super) async fn save_execution_state_for_input(
        &mut self,
        input_request: &Value,
    ) -> Result<(), AppError> {
        let question = input_request
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let context = input_request
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("Additional information needed")
            .to_string();

        let input_type = input_request
            .get("input_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();

        let options = input_request
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

        let default_value = input_request
            .get("default_value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let placeholder = input_request
            .get("placeholder")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let user_input_state = UserInputRequestState {
            question,
            context,
            input_type,
            options,
            default_value,
            placeholder,
        };

        let execution_state = AgentExecutionState {
            task_results: self
                .task_results
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
                .collect(),
            current_objective: self
                .current_objective
                .as_ref()
                .map(|o| serde_json::to_value(o).unwrap()),
            conversation_insights: self
                .conversation_insights
                .as_ref()
                .map(|c| serde_json::to_value(c).unwrap()),
            workflow_state: self.get_current_workflow_state(),
            pending_input_request: Some(user_input_state),
        };

        UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .with_execution_state(execution_state)
            .with_status(ExecutionContextStatus::WaitingForInput)
            .execute(&self.app_state)
            .await?;

        Ok(())
    }
}
