use models::{AnswerKind, AnswerValue, Choice, PendingQuestion, Question, QuestionAnswer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AskUserParams {
    pub questions: Vec<Question>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AnswerSubmission {
    pub answers: Vec<QuestionAnswer>,
}

pub fn validate_question_set(questions: &[Question]) -> Result<(), String> {
    if questions.is_empty() {
        return Err("ask_user must include at least one question".to_string());
    }
    let mut seen = std::collections::HashSet::new();
    for q in questions {
        if q.id.trim().is_empty() {
            return Err("question.id must be non-empty".to_string());
        }
        if !seen.insert(q.id.clone()) {
            return Err(format!("duplicate question id: {}", q.id));
        }
        if q.text.trim().is_empty() {
            return Err(format!("question {} has empty text", q.id));
        }
        validate_answer_kind(&q.id, &q.answer_kind)?;
    }
    Ok(())
}

fn validate_answer_kind(qid: &str, kind: &AnswerKind) -> Result<(), String> {
    match kind {
        AnswerKind::SingleChoice { choices, .. } => {
            if choices.is_empty() {
                return Err(format!(
                    "question {qid}: single_choice requires at least one choice"
                ));
            }
            ensure_unique_choice_values(qid, choices)?;
        }
        AnswerKind::MultiChoice {
            choices,
            min_selected,
            max_selected,
        } => {
            if choices.is_empty() {
                return Err(format!(
                    "question {qid}: multi_choice requires at least one choice"
                ));
            }
            ensure_unique_choice_values(qid, choices)?;
            if let (Some(min), Some(max)) = (min_selected, max_selected) {
                if min > max {
                    return Err(format!(
                        "question {qid}: min_selected ({min}) exceeds max_selected ({max})"
                    ));
                }
            }
        }
        AnswerKind::Number { min, max, .. } => {
            if let (Some(min), Some(max)) = (min, max) {
                if min > max {
                    return Err(format!(
                        "question {qid}: number min ({min}) exceeds max ({max})"
                    ));
                }
            }
        }
        AnswerKind::Date { min_date, max_date } => {
            for value in [min_date.as_deref(), max_date.as_deref()].into_iter().flatten() {
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
                    format!("question {qid}: date bound '{value}' must be ISO yyyy-mm-dd")
                })?;
            }
        }
        AnswerKind::Confirm {
            confirm_label,
            cancel_label,
        } => {
            if confirm_label.trim().is_empty() || cancel_label.trim().is_empty() {
                return Err(format!(
                    "question {qid}: confirm labels must be non-empty"
                ));
            }
        }
        AnswerKind::FreeText { max_length, .. } => {
            if let Some(max) = max_length {
                if *max == 0 {
                    return Err(format!("question {qid}: max_length must be > 0"));
                }
            }
        }
        AnswerKind::YesNo => {}
    }
    Ok(())
}

fn ensure_unique_choice_values(qid: &str, choices: &[Choice]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for choice in choices {
        if choice.value.trim().is_empty() {
            return Err(format!("question {qid}: choice value must be non-empty"));
        }
        if !seen.insert(choice.value.clone()) {
            return Err(format!(
                "question {qid}: duplicate choice value '{}'",
                choice.value
            ));
        }
    }
    Ok(())
}

pub fn validate_answers(
    pending: &PendingQuestion,
    submission: &AnswerSubmission,
) -> Result<(), String> {
    if submission.answers.len() != pending.questions.len() {
        return Err(format!(
            "expected {} answers, got {}",
            pending.questions.len(),
            submission.answers.len()
        ));
    }
    let mut answered = std::collections::HashSet::new();
    for answer in &submission.answers {
        if !answered.insert(answer.question_id.clone()) {
            return Err(format!(
                "duplicate answer for question_id '{}'",
                answer.question_id
            ));
        }
        let question = pending
            .questions
            .iter()
            .find(|q| q.id == answer.question_id)
            .ok_or_else(|| format!("unknown question_id '{}'", answer.question_id))?;
        validate_answer_value(&question.answer_kind, &answer.value)?;
    }
    Ok(())
}

fn validate_answer_value(kind: &AnswerKind, value: &AnswerValue) -> Result<(), String> {
    match (kind, value) {
        (AnswerKind::FreeText { max_length, .. }, AnswerValue::FreeText { value }) => {
            if value.is_empty() {
                return Err("free_text answer must be non-empty".to_string());
            }
            if let Some(max) = max_length {
                if value.chars().count() as u32 > *max {
                    return Err(format!("free_text answer exceeds max_length {max}"));
                }
            }
        }
        (
            AnswerKind::SingleChoice {
                choices,
                allow_other,
            },
            AnswerValue::SingleChoice { value },
        ) => {
            if value.trim().is_empty() {
                return Err("single_choice answer must be non-empty".to_string());
            }
            let known = choices.iter().any(|c| c.value == *value);
            if !known && !*allow_other {
                return Err(format!("'{value}' is not a valid choice"));
            }
        }
        (
            AnswerKind::MultiChoice {
                choices,
                min_selected,
                max_selected,
            },
            AnswerValue::MultiChoice { values },
        ) => {
            let count = values.len() as u32;
            if let Some(min) = min_selected {
                if count < *min {
                    return Err(format!("multi_choice requires at least {min} selections"));
                }
            }
            if let Some(max) = max_selected {
                if count > *max {
                    return Err(format!("multi_choice allows at most {max} selections"));
                }
            }
            let mut seen = std::collections::HashSet::new();
            for value in values {
                if !seen.insert(value.clone()) {
                    return Err(format!("duplicate selection '{value}'"));
                }
                if !choices.iter().any(|c| c.value == *value) {
                    return Err(format!("'{value}' is not a valid choice"));
                }
            }
        }
        (AnswerKind::YesNo, AnswerValue::YesNo { .. }) => {}
        (AnswerKind::Number { min, max, .. }, AnswerValue::Number { value }) => {
            if let Some(min) = min {
                if value < min {
                    return Err(format!("number {value} below min {min}"));
                }
            }
            if let Some(max) = max {
                if value > max {
                    return Err(format!("number {value} above max {max}"));
                }
            }
        }
        (AnswerKind::Date { min_date, max_date }, AnswerValue::Date { value }) => {
            let parsed = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| format!("date '{value}' must be ISO yyyy-mm-dd"))?;
            if let Some(min) = min_date {
                let bound = chrono::NaiveDate::parse_from_str(min, "%Y-%m-%d").unwrap();
                if parsed < bound {
                    return Err(format!("date {value} before min_date {min}"));
                }
            }
            if let Some(max) = max_date {
                let bound = chrono::NaiveDate::parse_from_str(max, "%Y-%m-%d").unwrap();
                if parsed > bound {
                    return Err(format!("date {value} after max_date {max}"));
                }
            }
        }
        (AnswerKind::Confirm { .. }, AnswerValue::Confirm { .. }) => {}
        _ => {
            return Err("answer kind does not match question kind".to_string());
        }
    }
    Ok(())
}
