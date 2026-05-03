use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnswerKind {
    FreeText {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
    },
    SingleChoice {
        choices: Vec<Choice>,
        #[serde(default)]
        allow_other: bool,
    },
    MultiChoice {
        choices: Vec<Choice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_selected: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_selected: Option<u32>,
    },
    YesNo,
    Number {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    Date {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_date: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_date: Option<String>,
    },
    Confirm {
        confirm_label: String,
        cancel_label: String,
    },
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub answer_kind: AnswerKind,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnswerValue {
    FreeText { value: String },
    SingleChoice { value: String },
    MultiChoice { values: Vec<String> },
    YesNo { value: bool },
    Number { value: f64 },
    Date { value: String },
    Confirm { accepted: bool },
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct QuestionAnswer {
    pub question_id: String,
    pub value: AnswerValue,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PendingQuestion {
    pub questions: Vec<Question>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub asked_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub asked_by_thread_id: i64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::utils::serde::i64_as_string_option"
    )]
    pub asked_by_assignment_id: Option<i64>,
}
