use serde_json::Value;
use shared::commands::{Command, GenerateEmbeddingCommand};
use shared::error::AppError;
use shared::models::{AgentExecutionContextMessage, MemoryEntry};
use shared::queries::{MessageSimilarityResult, Query, SearchMessagesBySimilarityQuery};
use shared::state::AppState;
use tiktoken_rs::cl100k_base;

const MAX_CONTEXT_TOKENS: usize = 150_000;
const RESERVED_TOKENS: usize = 10_000;

#[derive(Clone, Debug)]
pub struct ContextItem {
    pub content: String,
    pub source: String,
    pub relevance_score: f64,
    pub token_count: usize,
}

pub struct ContextAggregator {
    app_state: AppState,
    execution_context_id: i64,
}

impl ContextAggregator {
    pub fn new(app_state: AppState, execution_context_id: i64) -> Self {
        Self {
            app_state,
            execution_context_id,
        }
    }

    pub async fn aggregate_context(
        &self,
        query: &str,
        conversation_history: &[AgentExecutionContextMessage],
        memories: &[MemoryEntry],
        knowledge_base_results: &[Value],
    ) -> Result<Vec<ContextItem>, AppError> {
        let query_embedding = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;

        let similar_messages = self.search_similar_messages(&query_embedding).await?;

        let mut all_items = Vec::new();

        for result in similar_messages {
            let content = &result.message.content;
            let tokens = count_tokens(content);

            all_items.push(ContextItem {
                content: content.clone(),
                source: "message_history".to_string(),
                relevance_score: result.similarity,
                token_count: tokens,
            });
        }

        let recent_history: Vec<_> = conversation_history.iter().take(10).collect();

        for (idx, msg) in recent_history.iter().enumerate() {
            let tokens = count_tokens(&msg.content);
            let recency_score = 0.8 - (idx as f64 * 0.05);

            all_items.push(ContextItem {
                content: msg.content.clone(),
                source: "recent_conversation".to_string(),
                relevance_score: recency_score,
                token_count: tokens,
            });
        }

        for memory in memories {
            let tokens = count_tokens(&memory.content);

            all_items.push(ContextItem {
                content: memory.content.clone(),
                source: format!("memory_{:?}", memory.memory_type),
                relevance_score: memory.importance,
                token_count: tokens,
            });
        }


        for result in knowledge_base_results {
            if let Some(content) = result.get("content").and_then(|c| c.as_str()) {
                let tokens = count_tokens(content);
                let score = result
                    .get("similarity")
                    .and_then(|s| s.as_f64())
                    .unwrap_or(0.5);

                all_items.push(ContextItem {
                    content: content.to_string(),
                    source: "knowledge_base".to_string(),
                    relevance_score: score as f64,
                    token_count: tokens,
                });
            }
        }

        all_items.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());

        let selected_items =
            self.select_within_token_budget(all_items, MAX_CONTEXT_TOKENS - RESERVED_TOKENS);

        Ok(selected_items)
    }

    pub fn format_context_for_prompt(&self, items: &[ContextItem]) -> String {
        let mut sections = std::collections::HashMap::new();

        for item in items {
            sections
                .entry(item.source.clone())
                .or_insert_with(Vec::new)
                .push(item);
        }

        let mut formatted = String::new();

        if let Some(recent) = sections.get("recent_conversation") {
            formatted.push_str("## Recent Conversation\n");
            for item in recent {
                formatted.push_str(&format!("{}\n\n", item.content));
            }
        }

        if let Some(history) = sections.get("message_history") {
            formatted.push_str("## Relevant Message History\n");
            for item in history {
                formatted.push_str(&format!("{}\n\n", item.content));
            }
        }

        if let Some(memories) = sections.get("memory_Episodic") {
            formatted.push_str("## Episodic Memories\n");
            for item in memories {
                formatted.push_str(&format!("{}\n\n", item.content));
            }
        }

        if let Some(memories) = sections.get("memory_Semantic") {
            formatted.push_str("## Semantic Knowledge\n");
            for item in memories {
                formatted.push_str(&format!("{}\n\n", item.content));
            }
        }

        if let Some(memories) = sections.get("memory_Procedural") {
            formatted.push_str("## Procedural Knowledge\n");
            for item in memories {
                formatted.push_str(&format!("{}\n\n", item.content));
            }
        }


        if let Some(kb) = sections.get("knowledge_base") {
            formatted.push_str("## Knowledge Base\n");
            for item in kb {
                formatted.push_str(&format!("{}\n\n", item.content));
            }
        }

        formatted
    }

    async fn search_similar_messages(
        &self,
        query_embedding: &[f32],
    ) -> Result<Vec<MessageSimilarityResult>, AppError> {
        SearchMessagesBySimilarityQuery::new(self.execution_context_id, query_embedding.to_vec())
            .with_max_results(20)
            .with_min_similarity(0.7)
            .execute(&self.app_state)
            .await
    }

    fn select_within_token_budget(
        &self,
        mut items: Vec<ContextItem>,
        budget: usize,
    ) -> Vec<ContextItem> {
        let mut selected = Vec::new();
        let mut total_tokens = 0;

        let mut recent_count = 0;
        items.retain(|item| {
            if item.source == "recent_conversation" && recent_count < 5 {
                let new_total = total_tokens + item.token_count;
                if new_total <= budget {
                    total_tokens = new_total;
                    selected.push(item.clone());
                    recent_count += 1;
                    return false;
                }
            }
            true
        });

        for item in items {
            let new_total = total_tokens + item.token_count;
            if new_total <= budget {
                total_tokens = new_total;
                selected.push(item);
            } else {
                break;
            }
        }

        selected
    }
}

fn count_tokens(text: &str) -> usize {
    let bpe = cl100k_base().unwrap();
    bpe.encode_with_special_tokens(text).len()
}

