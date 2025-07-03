use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub memory_type: MemoryType,
    pub content: String,
    pub metadata: HashMap<String, Value>,
    pub importance: f32,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub embedding: Vec<f32>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum MemoryType {
    Working,
    Episodic,
    Semantic,
    Procedural,
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

#[derive(Clone)]
pub struct MemoryQuery {
    pub query: String,
    pub memory_types: Vec<MemoryType>,
    pub max_results: usize,
    pub min_importance: f32,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

#[derive(Clone)]
pub struct MemorySearchResult {
    pub entry: MemoryEntry,
    pub relevance_score: f32,
    pub similarity_score: f32,
}

#[derive(Serialize, Deserialize, clickhouse::Row)]
pub struct MemoryRecord {
    pub id: i64,
    pub deployment_id: i64,
    pub agent_id: i64,
    pub execution_context_id: i64,
    pub memory_type: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub importance: f32,
    pub access_count: i32,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub last_accessed_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct MemorySearchRecord {
    pub id: i64,
    pub content: String,
    pub score: f32,
    pub agent_id: i64,
    pub memory_type: String,
    pub importance: f32,
    pub access_count: i32,
}

impl From<MemoryEntry> for MemoryRecord {
    fn from(entry: MemoryEntry) -> Self {
        Self {
            id: entry.id,
            deployment_id: 0,
            agent_id: 0,
            execution_context_id: 0,
            memory_type: entry.memory_type.to_string(),
            content: entry.content,
            embedding: entry.embedding,
            importance: entry.importance,
            access_count: entry.access_count as i32,
            created_at: entry.created_at,
            last_accessed_at: entry.last_accessed,
        }
    }
}

impl From<MemoryRecord> for MemoryEntry {
    fn from(record: MemoryRecord) -> Self {
        let memory_type = match record.memory_type.as_str() {
            "working" => MemoryType::Working,
            "episodic" => MemoryType::Episodic,
            "semantic" => MemoryType::Semantic,
            "procedural" => MemoryType::Procedural,
            _ => MemoryType::Working,
        };

        Self {
            id: record.id,
            memory_type,
            content: record.content,
            metadata: HashMap::new(),
            importance: record.importance,
            created_at: record.created_at,
            last_accessed: record.last_accessed_at,
            access_count: record.access_count as u32,
            embedding: record.embedding,
        }
    }
}
