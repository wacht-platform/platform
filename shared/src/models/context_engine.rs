use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Search mode for hybrid search
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Semantic search only using vector embeddings
    Vector,
    /// Full-text search only using PostgreSQL text search
    FullText,
    /// Hybrid search combining vector and full-text with weights
    Hybrid {
        vector_weight: f32,
        text_weight: f32,
    },
}

/// Context engine tool for intelligent knowledge retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEngineTool {
    pub id: i64,
    pub name: String, // Always "context_engine"
    pub description: String,
    pub parameters_schema: Value, // JSON schema for parameters
}

/// Parameters for context engine tool calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEngineParams {
    pub action: ContextAction,
    pub query: String,
    pub filters: Option<ContextFilters>,
}

/// Available actions for context engine
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextAction {
    SearchKnowledgeBase { 
        kb_id: Option<i64> 
    },
    SearchDynamicContext { 
        context_type: Option<String> 
    },
    SearchMemories { 
        category: Option<String> 
    },
    SearchConversations { 
        context_id: Option<i64> 
    },
    SearchAll,
}

/// Filters for context searches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFilters {
    pub max_results: usize,
    pub min_relevance: f64,
    pub time_range: Option<TimeRange>,
    pub search_mode: SearchMode,
    pub boost_keywords: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Result from context engine search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchResult {
    pub source: ContextSource,
    pub content: String,
    pub relevance_score: f64,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    KnowledgeBase { kb_id: i64, document_id: i64 },
    DynamicContext { context_type: String },
    Memory { memory_id: i64, category: String },
    Conversation { conversation_id: i64 },
}

impl Default for ContextFilters {
    fn default() -> Self {
        Self {
            max_results: 10,
            min_relevance: 0.7,
            time_range: None,
            search_mode: SearchMode::Hybrid {
                vector_weight: 0.7,
                text_weight: 0.3,
            },
            boost_keywords: None,
        }
    }
}

impl Default for SearchMode {
    fn default() -> Self {
        SearchMode::Hybrid {
            vector_weight: 0.7,
            text_weight: 0.3,
        }
    }
}

impl ContextEngineTool {
    /// Create the standard context engine tool definition
    pub fn create_tool_definition(id: i64) -> Self {
        Self {
            id,
            name: "context_engine".to_string(),
            description: "Search for relevant context from knowledge bases, memories, conversations, and dynamic context. Use this tool when you need specific information not provided in the immediate context.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["search_knowledge_base", "search_dynamic_context", "search_memories", "search_conversations", "search_all"],
                                "description": "The type of search to perform"
                            },
                            "kb_id": {
                                "type": "integer",
                                "description": "Knowledge base ID (for search_knowledge_base)"
                            },
                            "context_type": {
                                "type": "string",
                                "description": "Context type (for search_dynamic_context)"
                            },
                            "category": {
                                "type": "string",
                                "enum": ["procedural", "semantic", "episodic"],
                                "description": "Memory category (for search_memories)"
                            },
                            "context_id": {
                                "type": "integer",
                                "description": "Context ID (for search_conversations)"
                            }
                        },
                        "required": ["type"]
                    },
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "filters": {
                        "type": "object",
                        "properties": {
                            "max_results": {
                                "type": "integer",
                                "description": "Maximum number of results to return",
                                "default": 10
                            },
                            "min_relevance": {
                                "type": "number",
                                "description": "Minimum relevance score (0.0-1.0)",
                                "default": 0.7
                            },
                            "time_range": {
                                "type": "object",
                                "properties": {
                                    "start": {
                                        "type": "string",
                                        "format": "date-time"
                                    },
                                    "end": {
                                        "type": "string",
                                        "format": "date-time"
                                    }
                                },
                                "required": ["start", "end"]
                            }
                        }
                    }
                },
                "required": ["action", "query"]
            }),
        }
    }
}