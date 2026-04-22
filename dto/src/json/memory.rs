use models::{ConversationContent, ConversationMessageType};
use serde::{Deserialize, Serialize};

// LLM Interaction DTOs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvaluationResponse {
    pub worth_storing: bool,
    pub confidence: f64,
    pub reasoning: String,
    pub suggested_tags: Vec<String>,
    pub retention_priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFormationDecision {
    pub should_store: bool,
    pub memory_type: Option<MemoryCategory>,
    pub importance_score: f64,
    pub reasoning: String,
    pub suggested_compression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRetrievalStrategy {
    pub memory_categories: Vec<MemoryCategory>,
    pub relevance_threshold: f64,
    pub time_window_days: Option<i32>,
    pub max_results: i32,
    pub search_approach: String,
}

// Request DTOs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateConversationRequest {
    pub id: i64,
    pub thread_id: i64,
    pub content: ConversationContent,
    pub message_type: ConversationMessageType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    Procedural,
    Semantic,
}

impl ToString for MemoryCategory {
    fn to_string(&self) -> String {
        match self {
            MemoryCategory::Procedural => "procedural".to_string(),
            MemoryCategory::Semantic => "semantic".to_string(),
        }
    }
}

impl MemoryCategory {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "procedural" => Some(MemoryCategory::Procedural),
            "semantic" => Some(MemoryCategory::Semantic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCitationMetricsRequest {
    pub item_id: i64,
    pub item_type: CitationItemType,
    pub relevance_delta: f64,
    pub usefulness_delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationItemType {
    Memory,
    Conversation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCreateConversationsRequest {
    pub conversations: Vec<CreateConversationRequest>,
}

// Response DTOs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationResponse {
    pub id: i64,
    pub thread_id: i64,
    pub timestamp: String,
    pub content: ConversationContent,
    pub message_type: ConversationMessageType,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResponse {
    pub id: i64,
    pub content: String,
    pub memory_category: String,
    pub created_at: String,
    pub updated_at: String,
}

// Query DTOs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchQuery {
    pub query: String,
    pub memory_categories: Option<Vec<MemoryCategory>>,
    pub thread_id: Option<i64>,
    pub limit: Option<i32>,
    pub min_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationHistoryQuery {
    pub thread_id: i64,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
    pub message_types: Option<Vec<ConversationMessageType>>,
}
