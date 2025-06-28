use super::AgentContext;
use shared::error::AppError;
use shared::state::AppState;

use serde_json::{json, Value};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Clone)]
pub struct MemoryManager {
    pub context: AgentContext,
    pub app_state: AppState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub memory_type: MemoryType,
    pub content: String,
    pub metadata: HashMap<String, Value>,
    pub importance: f32,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MemoryType {
    Working,    // Current conversation and immediate context
    Episodic,   // Specific events and experiences
    Semantic,   // Facts, knowledge, and learned patterns
    Procedural, // How to perform tasks and workflows
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryType::Working => write!(f, "working"),
            MemoryType::Episodic => write!(f, "episodic"),
            MemoryType::Semantic => write!(f, "semantic"),
            MemoryType::Procedural => write!(f, "procedural"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryQuery {
    pub query: String,
    pub memory_types: Vec<MemoryType>,
    pub max_results: usize,
    pub min_importance: f32,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
}



#[derive(Debug, Clone)]
pub struct MemorySearchResult {
    pub entry: MemoryEntry,
    pub relevance_score: f32,
    pub similarity_score: f32,
}

impl MemoryManager {
    pub fn new(context: AgentContext, app_state: AppState) -> Result<Self, AppError> {
        Ok(Self {
            context,
            app_state,
        })
    }

    /// Store a new memory entry
    pub async fn store_memory(&self, memory_type: MemoryType, content: &str, metadata: HashMap<String, Value>, importance: f32) -> Result<String, AppError> {
        let memory_id = self.generate_memory_id();
        
        // Generate embedding for the content
        let embedding = self.app_state.embedding_service.generate_embedding(content.to_string()).await?;
        
        let memory_entry = MemoryEntry {
            id: memory_id.clone(),
            memory_type,
            content: content.to_string(),
            metadata,
            importance,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            embedding: Some(embedding),
        };

        // Store in the execution context memory field
        self.store_memory_entry(&memory_entry).await?;
        
        Ok(memory_id)
    }

    /// Retrieve memories based on query and filters (simple interface)


    /// Retrieve memories based on a query (detailed interface with scores)
    pub async fn search_memories(&self, query: &MemoryQuery) -> Result<Vec<MemorySearchResult>, AppError> {
        // Generate embedding for the query
        let query_embedding = self.app_state.embedding_service.generate_embedding(query.query.clone()).await?;
        
        // Get all stored memories
        let stored_memories = self.get_stored_memories().await?;
        
        let mut results = Vec::new();
        
        for memory in stored_memories {
            // Filter by memory type
            if !query.memory_types.is_empty() && !self.memory_type_matches(&memory.memory_type, &query.memory_types) {
                continue;
            }
            
            // Filter by importance
            if memory.importance < query.min_importance {
                continue;
            }
            
            // Filter by time range
            if let Some((start, end)) = query.time_range {
                if memory.created_at < start || memory.created_at > end {
                    continue;
                }
            }
            
            // Calculate relevance scores
            let text_relevance = self.calculate_text_relevance(&memory.content, &query.query);
            let semantic_similarity = if let Some(ref memory_embedding) = memory.embedding {
                self.calculate_cosine_similarity(&query_embedding, memory_embedding)
            } else {
                0.0
            };
            
            // Combined relevance score (weighted)
            let relevance_score = (text_relevance * 0.3) + (semantic_similarity * 0.7);
            
            results.push(MemorySearchResult {
                entry: memory,
                relevance_score,
                similarity_score: semantic_similarity,
            });
        }
        
        // Sort by relevance score
        results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));
        
        // Limit results
        results.truncate(query.max_results);
        
        Ok(results)
    }

    /// Update memory importance based on usage patterns


    /// Consolidate memories by merging similar ones
    pub async fn consolidate_memories(&self, similarity_threshold: f32) -> Result<usize, AppError> {
        let memories = self.get_stored_memories().await?;
        let mut consolidated_memories: Vec<MemoryEntry> = Vec::new();
        let mut merged_count = 0;

        for memory in memories.iter() {
            let mut should_merge = false;

            // Check if this memory should be merged with any existing consolidated memory
            for consolidated in &mut consolidated_memories {
                if let Some(ref memory_embedding) = memory.embedding {
                    if let Some(ref consolidated_embedding) = consolidated.embedding {
                        let similarity = self.calculate_cosine_similarity(memory_embedding, consolidated_embedding);

                        if similarity > similarity_threshold &&
                           std::mem::discriminant(&memory.memory_type) == std::mem::discriminant(&consolidated.memory_type) {
                            // Merge memories
                            consolidated.content = format!("{}\n\n{}", consolidated.content, memory.content);
                            consolidated.importance = (consolidated.importance + memory.importance) / 2.0;
                            consolidated.access_count += memory.access_count;

                            // Merge metadata
                            for (key, value) in &memory.metadata {
                                consolidated.metadata.insert(key.clone(), value.clone());
                            }

                            should_merge = true;
                            merged_count += 1;
                            break;
                        }
                    }
                }
            }

            if !should_merge {
                consolidated_memories.push(memory.clone());
            }
        }

        // Store consolidated memories
        self.store_all_memories(&consolidated_memories).await?;

        Ok(merged_count)
    }

    /// Forget low-importance memories to manage memory size
    pub async fn forget_memories(&self, max_memories: usize, min_importance: f32) -> Result<usize, AppError> {
        let mut memories = self.get_stored_memories().await?;
        let initial_count = memories.len();
        
        // Remove memories below minimum importance
        memories.retain(|m| m.importance >= min_importance);
        
        // If still too many, remove oldest low-importance memories
        if memories.len() > max_memories {
            // Sort by importance (descending) and recency (descending)
            memories.sort_by(|a, b| {
                let importance_cmp = b.importance.partial_cmp(&a.importance).unwrap_or(std::cmp::Ordering::Equal);
                if importance_cmp == std::cmp::Ordering::Equal {
                    b.last_accessed.cmp(&a.last_accessed)
                } else {
                    importance_cmp
                }
            });
            
            memories.truncate(max_memories);
        }
        
        let forgotten_count = initial_count - memories.len();
        
        // Store remaining memories
        self.store_all_memories(&memories).await?;
        
        Ok(forgotten_count)
    }

    /// Get memory statistics
    pub async fn get_memory_stats(&self) -> Result<Value, AppError> {
        let memories = self.get_stored_memories().await?;
        
        let mut stats = HashMap::new();
        let mut type_counts = HashMap::new();
        let mut total_importance = 0.0;
        let mut total_access_count = 0;
        
        for memory in &memories {
            let type_name = format!("{:?}", memory.memory_type);
            *type_counts.entry(type_name).or_insert(0) += 1;
            total_importance += memory.importance;
            total_access_count += memory.access_count;
        }
        
        stats.insert("total_memories".to_string(), json!(memories.len()));
        stats.insert("memory_types".to_string(), json!(type_counts));
        stats.insert("average_importance".to_string(), json!(if memories.is_empty() { 0.0 } else { total_importance / memories.len() as f32 }));
        stats.insert("total_access_count".to_string(), json!(total_access_count));
        stats.insert("agent_id".to_string(), json!(self.context.agent_id));
        
        Ok(json!(stats))
    }

    // Helper methods
    fn generate_memory_id(&self) -> String {
        format!("mem_{}_{}", self.context.agent_id, Utc::now().timestamp_nanos_opt().unwrap_or(0))
    }

    fn memory_type_matches(&self, memory_type: &MemoryType, query_types: &[MemoryType]) -> bool {
        query_types.iter().any(|qt| std::mem::discriminant(memory_type) == std::mem::discriminant(qt))
    }

    fn calculate_text_relevance(&self, content: &str, query: &str) -> f32 {
        let content_lower = content.to_lowercase();
        let query_lower = query.to_lowercase();
        
        // Simple text matching score
        let mut score = 0.0;
        
        // Exact match
        if content_lower.contains(&query_lower) {
            score += 0.5;
        }
        
        // Word-level matching
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let content_words: Vec<&str> = content_lower.split_whitespace().collect();
        
        let mut matched_words = 0;
        for query_word in &query_words {
            if content_words.iter().any(|cw| cw.contains(query_word)) {
                matched_words += 1;
            }
        }
        
        if !query_words.is_empty() {
            score += (matched_words as f32 / query_words.len() as f32) * 0.5;
        }
        
        score.clamp(0.0, 1.0)
    }

    fn calculate_cosine_similarity(&self, vec1: &[f32], vec2: &[f32]) -> f32 {
        if vec1.len() != vec2.len() {
            return 0.0;
        }
        
        let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
        let norm1: f32 = vec1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm2: f32 = vec2.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm1 == 0.0 || norm2 == 0.0 {
            0.0
        } else {
            dot_product / (norm1 * norm2)
        }
    }

    // Storage methods
    async fn store_memory_entry(&self, memory: &MemoryEntry) -> Result<(), AppError> {
        // Store in Qdrant for vector search
        if let Some(embedding) = &memory.embedding {
            self.app_state.qdrant_service.store_memory(
                memory.id.parse::<i64>().unwrap_or(0),
                self.context.agent_id,
                self.context.deployment_id,
                self.get_execution_context_id().await?,
                &memory.memory_type.to_string(),
                &memory.content,
                embedding.clone(),
                memory.importance,
                memory.metadata.clone(),
            ).await?;
        }

        // Also store in database for backup/persistence
        let mut memories = self.get_stored_memories().await?;
        memories.retain(|m| m.id != memory.id);
        memories.push(memory.clone());
        self.store_all_memories(&memories).await
    }

    async fn get_stored_memories(&self) -> Result<Vec<MemoryEntry>, AppError> {
        use shared::queries::{GetExecutionContextsByAgentQuery, Query};

        // Get the latest execution context for this agent
        let contexts = GetExecutionContextsByAgentQuery::new(self.context.agent_id, self.context.deployment_id)
            .with_limit(1)
            .execute(&self.app_state).await?;

        if let Some(context) = contexts.first() {
            // Deserialize the memory field JSON into Vec<MemoryEntry>
            if let Ok(memories) = serde_json::from_value::<Vec<MemoryEntry>>(context.memory.clone()) {
                Ok(memories)
            } else {
                // If deserialization fails, return empty vector
                Ok(Vec::new())
            }
        } else {
            // No execution context found, return empty vector
            Ok(Vec::new())
        }
    }

    async fn store_all_memories(&self, memories: &[MemoryEntry]) -> Result<(), AppError> {
        use shared::queries::{GetExecutionContextsByAgentQuery, UpdateExecutionContextQuery, Query};

        // Get the latest execution context for this agent
        let contexts = GetExecutionContextsByAgentQuery::new(self.context.agent_id, self.context.deployment_id)
            .with_limit(1)
            .execute(&self.app_state).await?;

        if let Some(context) = contexts.first() {
            // Serialize the memories to JSON
            let memory_json = serde_json::to_value(memories)
                .map_err(|e| AppError::Internal(format!("Failed to serialize memories: {}", e)))?;

            // Update the execution context memory field in the database
            UpdateExecutionContextQuery::new(context.id, self.context.deployment_id)
                .with_memory(memory_json)
                .execute(&self.app_state).await?;

            println!("Stored {} memories for agent {} in execution context {}",
                memories.len(), self.context.agent_id, context.id);
        } else {
            // No execution context found - this shouldn't happen in normal operation
            eprintln!("Warning: No execution context found for agent {} when storing memories",
                self.context.agent_id);
        }

        Ok(())
    }

    async fn get_execution_context_id(&self) -> Result<i64, AppError> {
        use shared::queries::{GetExecutionContextsByAgentQuery, Query};

        // Get the latest execution context for this agent
        let contexts = GetExecutionContextsByAgentQuery::new(self.context.agent_id, self.context.deployment_id)
            .with_limit(1)
            .execute(&self.app_state).await?;

        if let Some(context) = contexts.first() {
            Ok(context.id)
        } else {
            // If no execution context exists, return a default ID
            Ok(0)
        }
    }
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::Working => "working",
            MemoryType::Episodic => "episodic", 
            MemoryType::Semantic => "semantic",
            MemoryType::Procedural => "procedural",
        }
    }
    
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "working" => Some(MemoryType::Working),
            "episodic" => Some(MemoryType::Episodic),
            "semantic" => Some(MemoryType::Semantic),
            "procedural" => Some(MemoryType::Procedural),
            _ => None,
        }
    }
}
