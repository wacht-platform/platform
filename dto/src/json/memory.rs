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
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub content: ConversationContent,
    pub message_type: ConversationMessageType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    Procedural,
    Semantic,
    Fact,
    Preference,
    Observation,
    ConversationSummary,
}

impl ToString for MemoryCategory {
    fn to_string(&self) -> String {
        match self {
            MemoryCategory::Procedural => "procedural".to_string(),
            MemoryCategory::Semantic => "semantic".to_string(),
            MemoryCategory::Fact => "fact".to_string(),
            MemoryCategory::Preference => "preference".to_string(),
            MemoryCategory::Observation => "observation".to_string(),
            MemoryCategory::ConversationSummary => "conversation_summary".to_string(),
        }
    }
}

impl MemoryCategory {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "procedural" => Some(MemoryCategory::Procedural),
            "semantic" => Some(MemoryCategory::Semantic),
            "fact" => Some(MemoryCategory::Fact),
            "preference" => Some(MemoryCategory::Preference),
            "observation" => Some(MemoryCategory::Observation),
            "conversation_summary" => Some(MemoryCategory::ConversationSummary),
            _ => None,
        }
    }

    /// Returns the default retrieval weight boost for this category.
    /// Higher weight = more likely to surface in results.
    pub fn retrieval_weight(&self) -> f64 {
        match self {
            Self::Fact => 1.3,
            Self::Preference => 1.2,
            Self::Observation => 1.1,
            Self::ConversationSummary => 0.9,
            Self::Semantic => 1.0,
            Self::Procedural => 1.0,
        }
    }

    /// Human-readable label for tool descriptions.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Semantic => "fact or decision",
            Self::Procedural => "procedure or workflow",
            Self::Fact => "factual statement",
            Self::Preference => "user preference",
            Self::Observation => "event or observation",
            Self::ConversationSummary => "conversation summary",
        }
    }

    /// Hint about content structure for the agent.
    pub fn content_hint(&self) -> &'static str {
        match self {
            Self::Semantic => "A statement of fact, decision, or constraint.",
            Self::Procedural => "Validated steps to accomplish a recurring task.",
            Self::Fact => "A short, specific fact — e.g. \"User's timezone is UTC+2\".",
            Self::Preference => "A preference or setting — e.g. \"User prefers verbose output\".",
            Self::Observation => "What happened, the outcome, and why it matters — e.g. \"Build failed because X; rerun with Y flag\".",
            Self::ConversationSummary => "A condensed summary of what was discussed, decided, or produced in a conversation.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCitationMetricsRequest {
    #[serde(with = "models::utils::serde::i64_as_string")]
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
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub timestamp: String,
    pub content: ConversationContent,
    pub message_type: ConversationMessageType,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
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
    #[serde(default, with = "models::utils::serde::i64_as_string_option")]
    pub thread_id: Option<i64>,
    pub limit: Option<i32>,
    pub min_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationHistoryQuery {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
    pub message_types: Option<Vec<ConversationMessageType>>,
}
