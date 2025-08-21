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
    pub importance: f64,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub embedding: Vec<f32>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
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
    pub min_importance: f64,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

#[derive(Clone)]
pub struct MemorySearchResult {
    pub entry: MemoryEntry,
    pub relevance_score: f64,
    pub similarity_score: f64,
}
