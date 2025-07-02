use crate::agentic::MessageParser;

use super::{AgentContext, MemoryEntry, MemoryType};
use futures::StreamExt;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::state::AppState;

#[derive(Debug, Clone)]
pub struct AcknowledgmentRequest {
    pub user_message: String,
    pub conversation_history: Vec<ChatMessage>,
    pub memories: Vec<MemoryEntry>,
    pub agent_context: AgentContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgmentResponse {
    pub acknowledgment_message: String,
    pub further_action_required: bool,
    pub reasoning: String,
}

pub struct AcknowledgmentEngine {
    app_state: AppState,
}

impl AcknowledgmentEngine {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn generate_acknowledgment(
        &self,
        request: AcknowledgmentRequest,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<AcknowledgmentResponse, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-flash")
            .max_tokens(4000)
            .temperature(0.3)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let tools_info = self.format_tools_info(&request.agent_context);
        let workflows_info = self.format_workflows_info(&request.agent_context);
        let knowledge_bases_info = self.format_knowledge_bases_info(&request.agent_context);
        let memories_info = self.format_memories_info(&request.memories);

        let system_prompt = format!(
            r#"You are an intelligent AI agent greeting system. Your role is to:

1. **Acknowledge** the user's message with a brief message related to user's message, provide a conversational response as required!
2. **Analyze** whether further action is required to complete the request
3. **Provide reasoning** for your decision

## Available Capabilities:

### Tools:
{}

### Workflows:
{}

### Knowledge Bases:
{}

### Recent Memories:
{}

## Response Format:
You must respond with XML in this exact format:

<response>
<message>Brief acknowledgment message</message>
<further_action_required>true/false</further_action_required>
<reasoning>Brief explanation of why action is/isn't needed</reasoning>
</response>

## Guidelines:
- Keep acknowledgment message brief and professional unless otherwise required
- Don't lie, don't make up information or facts that aren't true
- Don't send incomplete messages
- Set further_action_required to false only if the request is purely informational or already answered or you are able to answer it yourself
- further_action_required will trigger a big thinking reasoning and answering loop, if we must collect a message from user immediately, it's best to set this as false
- You have to be conversational with the user, make sure you respect user's request as much as possible, at the same time if the user presents a request that you think is incomplete, you can ask the user for more information
- Be conservative - if unsure, mark as requiring action"#,
            tools_info, workflows_info, knowledge_bases_info, memories_info
        );

        let conversation_context = self.prepare_conversation_context(
            &request.conversation_history,
            &request.user_message,
            200_000,
        )?;

        let full_prompt = format!(
            "{}\n\n{}\n\nCurrent request: {}",
            system_prompt, conversation_context, request.user_message
        );

        let messages = vec![ChatMessage::user().content(&full_prompt).build()];

        let response_text = {
            let mut res = String::new();
            let mut parser = MessageParser::new();
            let mut stream = llm.chat_stream(&messages).await?;

            while let Some(Ok(token)) = stream.next().await {
                res.push_str(&token);

                if let Some(content) = parser.parse(&token) {
                    let _ = channel.send(StreamEvent::Token(content)).await;
                }
            }

            res
        };

        self.parse_acknowledgment_response(&response_text)
    }

    fn format_tools_info(&self, context: &AgentContext) -> String {
        if context.tools.is_empty() {
            return "No tools available".to_string();
        }

        context
            .tools
            .iter()
            .map(|tool| {
                format!(
                    "- {}: {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("No description")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_workflows_info(&self, context: &AgentContext) -> String {
        if context.workflows.is_empty() {
            return "No workflows available".to_string();
        }

        context
            .workflows
            .iter()
            .map(|workflow| {
                format!(
                    "- {}: {}",
                    workflow.name,
                    workflow.description.as_deref().unwrap_or("No description")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_knowledge_bases_info(&self, context: &AgentContext) -> String {
        if context.knowledge_bases.is_empty() {
            return "No knowledge bases available".to_string();
        }

        context
            .knowledge_bases
            .iter()
            .map(|kb| {
                format!(
                    "- {}: {}",
                    kb.name,
                    kb.description.as_deref().unwrap_or("No description")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_memories_info(&self, memories: &[MemoryEntry]) -> String {
        if memories.is_empty() {
            return "No recent memories".to_string();
        }

        memories
            .iter()
            .take(5)
            .map(|memory| {
                format!(
                    "- [{}] {} (importance: {:.2})",
                    match memory.memory_type {
                        MemoryType::Episodic => "Episode",
                        MemoryType::Semantic => "Fact",
                        MemoryType::Procedural => "Process",
                        MemoryType::Working => "Working",
                    },
                    memory.content,
                    memory.importance
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn prepare_conversation_context(
        &self,
        _conversation_history: &[ChatMessage],
        current_message: &str,
        _max_tokens: usize,
    ) -> Result<String, AppError> {
        // For now, we'll just include the current message
        // TODO: Implement proper conversation history parsing when ChatMessage structure is clarified
        let context = format!("Current Request: {}\n\n", current_message);
        Ok(context)
    }

    fn parse_acknowledgment_response(
        &self,
        response: &str,
    ) -> Result<AcknowledgmentResponse, AppError> {
        // Simple regex-based XML parsing for acknowledgment response
        let message = self.extract_xml_content(response, "message")?;
        let further_action_str = self
            .extract_xml_content(response, "further_action_required")
            .unwrap_or_else(|_| "true".to_string());
        let reasoning = self
            .extract_xml_content(response, "reasoning")
            .unwrap_or_else(|_| "No reasoning provided".to_string());

        let further_action_required = further_action_str.to_lowercase() == "true";

        Ok(AcknowledgmentResponse {
            acknowledgment_message: message,
            further_action_required,
            reasoning,
        })
    }

    fn extract_xml_content(&self, xml: &str, tag: &str) -> Result<String, AppError> {
        let start_tag = format!("<{}>", tag);
        let end_tag = format!("</{}>", tag);

        if let Some(start_pos) = xml.find(&start_tag) {
            let content_start = start_pos + start_tag.len();
            if let Some(end_pos) = xml[content_start..].find(&end_tag) {
                let content = xml[content_start..content_start + end_pos].trim();
                return Ok(content.to_string());
            }
        }

        Err(AppError::Internal(format!(
            "Could not find {} tag in XML response",
            tag
        )))
    }
}
