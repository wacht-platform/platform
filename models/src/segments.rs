use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Segment {
    #[serde(default, with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub segment_type: String,
}

#[derive(Serialize, Deserialize, FromRow)]
pub struct AnalyzedEntity {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub name: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AnalyzeRule {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub segment_id: i64,
    pub operator: String, // "include" | "exclude"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentOperator {
    Include,
    Exclude,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StringOperator {
    Eq,
    Contains,
    StartsWith,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumericOperator {
    Eq,
    Gt,
    Lt,
    Gte,
    Lte,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")] // segment, field, metadata, metric
pub enum AnalysisFilter {
    Segment {
        #[serde(with = "crate::utils::serde::i64_as_string")]
        id: i64,
        operator: SegmentOperator,
    },
    Field {
        field: String,
        operator: StringOperator,
        value: String,
    },
    Metadata {
        key: String,
        operator: StringOperator,
        value: String,
    },
    Metric {
        metric: String,
        operator: NumericOperator,
        value: i64,
    },
}
