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
            self.ctx.app_state.sf.next_id()? as i64,
            self.ctx.context_id,
            content,
            message_type,
        );
        command.execute(&self.ctx.app_state).await
    }

    pub(super) async fn store_user_message(
        &self,
        message: String,
        images: Option<Vec<dto::json::agent_executor::ImageData>>,
    ) -> Result<ConversationRecord, AppError> {
        let _model_images = if let Some(imgs) = images {
            let mut uploaded_images = Vec::new();

            for img in imgs {
                use base64::{engine::general_purpose::STANDARD, Engine};
                let bytes = STANDARD.decode(&img.data).map_err(|e| {
                    AppError::BadRequest(format!("Invalid base64 image data: {}", e))
                })?;

                let file_extension = img.mime_type.split('/').last().unwrap_or("png");
                let filename = format!("{}.{}", self.ctx.app_state.sf.next_id()?, file_extension);

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
            self.ctx.app_state.sf.next_id()? as i64,
            self.ctx.context_id,
            ConversationContent::UserMessage {
                message,
                sender_name: None,
                files: None,
            },
            ConversationMessageType::UserMessage,
        );
        let conversation = command.execute(&self.ctx.app_state).await?;

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
                    if let ConversationContent::UserMessage {
                        message,
                        files,
                        sender_name,
                    } = &conv.content
                    {
                        let mut parts = vec![json!({
                            "text": message
                        })];

                        if let Some(file_list) = files {
                            for file in file_list {
                                let attachment_note = if file.mime_type.starts_with("image/") {
                                    format!(
                                        "[Attached image: {} ({}). Call read_image(path=\"{}\") to analyze it.]",
                                        file.filename, file.url, file.url
                                    )
                                } else {
                                    format!("\n[Attached: {} ({})]", file.filename, file.url)
                                };
                                parts.push(json!({ "text": attachment_note }));
                            }
                        }

                        let mut entry = json!({
                            "role": "user",
                            "parts": parts,
                            "timestamp": conv.created_at,
                            "type": conv.message_type,
                        });
                        if let Some(ref name) = sender_name {
                            entry["sender"] = json!(name);
                        }
                        if let Some(ref meta) = conv.metadata {
                            entry["metadata"] = meta.clone();
                        }
                        history.push(entry);
                    } else {
                        let mut entry = json!({
                            "role": "user",
                            "content": self.extract_conversation_content(&conv.content),
                            "timestamp": conv.created_at,
                            "type": conv.message_type,
                        });
                        if let Some(ref meta) = conv.metadata {
                            entry["metadata"] = meta.clone();
                        }
                        history.push(entry);
                    }
                    i += 1;
                }
                ConversationMessageType::ActionExecutionResult => {
                    let is_recent = i + 10 >= self.conversations.len();
                    let mut content_value =
                        serde_json::to_value(&conv.content).unwrap_or_else(|_| json!({}));
                    let mut inline_parts: Vec<Value> = Vec::new();

                    if let Some(actual_results) = content_value
                        .get_mut("task_execution")
                        .and_then(|v| v.get_mut("actual_result"))
                        .and_then(|v| v.as_array_mut())
                    {
                        for item in actual_results.iter_mut() {
                            let Some(result_obj) = item.get_mut("result") else {
                                continue;
                            };

                            if !is_recent {
                                let approx_tokens = serde_json::to_string(result_obj)
                                    .map(|s| s.len() / 4)
                                    .unwrap_or(0);
                                if approx_tokens > 2000 {
                                    if let Some(map) = result_obj.as_object_mut() {
                                        map.remove("data");
                                        map.insert("data_omitted".to_string(), json!(true));
                                    }
                                }
                                continue;
                            }

                            let tool_name = result_obj
                                .get("tool_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            if tool_name != "read_image" {
                                continue;
                            }

                            let path = result_obj
                                .get("data")
                                .and_then(|v| v.get("path"))
                                .and_then(|v| v.as_str());
                            let mime_type = result_obj
                                .get("data")
                                .and_then(|v| v.get("mime_type"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("application/octet-stream");

                            if let Some(path) = path {
                                if let Ok(bytes) = self.filesystem.read_file_bytes(path).await {
                                    use base64::{engine::general_purpose::STANDARD, Engine};
                                    let base64_data = STANDARD.encode(bytes);
                                    inline_parts.push(json!({
                                        "inline_data": {
                                            "mime_type": mime_type,
                                            "data": base64_data
                                        }
                                    }));
                                }
                            }
                        }
                    }

                    let serialized = serde_json::to_string(&content_value).unwrap_or_default();
                    let mut parts = vec![json!({ "text": serialized })];
                    parts.extend(inline_parts);

                    let mut entry = json!({
                        "role": self.map_conversation_type_to_role(&conv.message_type),
                        "parts": parts,
                        "timestamp": conv.created_at,
                        "type": conv.message_type,
                    });
                    if let Some(ref meta) = conv.metadata {
                        entry["metadata"] = meta.clone();
                    }
                    history.push(entry);
                    i += 1;
                }
                _ => {
                    let mut entry = json!({
                        "role": self.map_conversation_type_to_role(&conv.message_type),
                        "content": self.extract_conversation_content(&conv.content),
                        "timestamp": conv.created_at,
                        "type": conv.message_type,
                    });
                    if let Some(ref meta) = conv.metadata {
                        entry["metadata"] = meta.clone();
                    }
                    history.push(entry);
                    i += 1;
                }
            }
        }

        history
    }

    pub(super) fn map_conversation_type_to_role(
        &self,
        msg_type: &ConversationMessageType,
    ) -> &'static str {
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

        memory
    }

    pub(super) fn extract_conversation_content(&self, content: &ConversationContent) -> String {
        match content {
            ConversationContent::UserMessage { message, .. } => message.clone(),
            ConversationContent::AssistantAcknowledgment {
                acknowledgment_message,
                further_action_required,
                reasoning,
                ..
            } => {
                let val = json!({
                    "next_step": "acknowledge",
                    "reasoning": reasoning,
                    "acknowledgment": {
                        "message": acknowledgment_message,
                        "further_action_required": further_action_required
                    }
                });
                val.to_string()
            }
            ConversationContent::AgentResponse { response, .. } => response.clone(),
            ConversationContent::UserInputRequest { question, .. } => question.clone(),
            ConversationContent::SystemDecision {
                step, reasoning, ..
            } => {
                format!("System Decision (Step: {}): {}", step, reasoning)
            }
            ConversationContent::ActionExecutionResult {
                ..
            } => serde_json::to_string(content).unwrap_or_default(),
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
            current_objective: self
                .current_objective
                .as_ref()
                .map(|o| serde_json::to_value(o).unwrap()),
            conversation_insights: self
                .conversation_insights
                .as_ref()
                .map(|c| serde_json::to_value(c).unwrap()),
            supervisor_mode_active: self.supervisor_mode_active,
            supervisor_task_board: self.supervisor_task_board.clone(),
            deep_think_mode_active: self.deep_think_mode_active,
            deep_think_used: self.deep_think_used,
            pending_input_request: Some(user_input_state),
        };

        UpdateExecutionContextQuery::new(self.ctx.context_id, self.ctx.agent.deployment_id)
            .with_execution_state(execution_state)
            .with_status(ExecutionContextStatus::WaitingForInput)
            .execute(&self.ctx.app_state)
            .await?;

        Ok(())
    }
}
