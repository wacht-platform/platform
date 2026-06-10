use common::error::AppError;
use serde_json::json;
use std::collections::BTreeMap;

use crate::executor::core::AgentExecutor;
use crate::llm::{
    NativeToolDefinition, SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest,
};

// (name, rendered title, description, required)
const SECTIONS: &[(&str, &str, &str, bool)] = &[
    (
        "objective",
        "Objective",
        "what this window of work was about and for whom",
        true,
    ),
    (
        "actions",
        "Actions",
        "what was actually done, in order — the condensed thought/act trail",
        true,
    ),
    (
        "outcomes",
        "Outcomes",
        "results and current state: deliverables, exact paths/IDs, and payload content worth preserving verbatim (drafted text, file contents, query results)",
        true,
    ),
    (
        "decisions",
        "Decisions",
        "key decisions and user corrections, verbatim where wording matters",
        false,
    ),
    (
        "errors_open",
        "Errors & Open Work",
        "exact error strings, any [tool_failure] from the window with no later fix (verbatim), and genuinely open work at window end",
        false,
    ),
];

const MAX_TURNS: usize = 30;

const SYSTEM_PROMPT: &str = r#"# compaction_summarizer
[identity]
role = "summarization worker compacting an execution window into a structured durable summary"
loop = "one tool call per turn; the harness replays the window, your current draft, your last tool result, and the turn counter each turn"
turn_budget = "spend turns proportionally to window size — a large window deserves more section passes and more memory extraction; finalize early only when the window is genuinely thin"
stakes = "the assembled sections become the ONLY surviving record of this window — anything not captured is lost"

[method]
order = "fill sections one at a time, most important first: objective → actions → outcomes → decisions → errors_open"
revise = "call write_section again on a section to replace it once you know more"
evidence = "preserve exact paths, IDs, error strings, and user corrections verbatim; no speculation, no padding"
carry_forward = "every [tool_failure] in the window with no later record fixing it MUST appear verbatim in errors_open"

[memories]
when = "the window holds a durable, reusable fact (root cause, working procedure, recurring failure signature, user preference) future runs should not rediscover"
dedupe = "ALWAYS search_memories first; if a similar memory exists, do not save"
volume = "no fixed cap — one save per distinct durable fact; a large window may hold many; stop when the window has no more, not at a quota"
quality = "each memory must stand alone and be specific; skip marginal or window-local facts"

[finalize]
when = "objective, actions, and outcomes are filled and accurate"
forbidden = "narrating this process or referencing these instructions in any section""#;

fn summarizer_tools() -> Vec<NativeToolDefinition> {
    let section_names: Vec<&str> = SECTIONS.iter().map(|(name, ..)| *name).collect();
    let section_docs = SECTIONS
        .iter()
        .map(|(name, _, desc, required)| {
            format!(
                "`{name}`{}: {desc}",
                if *required { " (required)" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    vec![
        NativeToolDefinition {
            name: "write_section".to_string(),
            description: format!(
                "Write or replace one summary section. Sections: {section_docs}."
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "section": { "type": "string", "enum": section_names },
                    "content": { "type": "string", "description": "Full replacement content for the section. Markdown bullets preferred." }
                },
                "required": ["section", "content"]
            }),
        },
        NativeToolDefinition {
            name: "search_memories".to_string(),
            description: "Search the agent's durable memories. Call before save_memory to avoid duplicates."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
        },
        NativeToolDefinition {
            name: "save_memory".to_string(),
            description: "Save one durable memory extracted from the window. Only after search_memories found nothing similar. One distinct fact per save; as many saves as the window genuinely holds."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "The durable fact, self-contained and specific." },
                    "observation": { "type": "string", "description": "One line on where in the window this came from." }
                },
                "required": ["content"]
            }),
        },
        NativeToolDefinition {
            name: "finalize".to_string(),
            description: "Assemble the sections into the final summary and end. Rejected while a required section is empty."
                .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
    ]
}

fn toml_value(body: &str) -> String {
    format!("\"\"\"\n{}\n\"\"\"", body.replace("\"\"\"", "'''"))
}

fn render_draft(sections: &BTreeMap<&'static str, String>) -> String {
    let body = SECTIONS
        .iter()
        .map(|(name, _, _, required)| {
            let value = match sections.get(name).map(String::as_str) {
                Some(body) => toml_value(body),
                None if *required => "\"\" # empty — required".to_string(),
                None => "\"\" # empty".to_string(),
            };
            format!("{name} = {value}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("[compacted_window]\n{body}")
}

fn required_missing(sections: &BTreeMap<&'static str, String>) -> Vec<&'static str> {
    SECTIONS
        .iter()
        .filter(|(name, _, _, required)| {
            *required
                && sections
                    .get(name)
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
        })
        .map(|(name, ..)| *name)
        .collect()
}

fn assemble(sections: &BTreeMap<&'static str, String>) -> String {
    let body = SECTIONS
        .iter()
        .filter_map(|(name, ..)| {
            sections
                .get(name)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|body| format!("{name} = {}", toml_value(body)))
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("[compacted_window]\n{body}")
}

impl AgentExecutor {
    pub(crate) async fn run_agentic_summary(
        &mut self,
        window_label: &str,
        window_text: &str,
    ) -> Result<String, AppError> {
        let tools = summarizer_tools();
        let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        let mut sections: BTreeMap<&'static str, String> = BTreeMap::new();
        let mut feedback = "(no tool calls yet — start by writing `objective`)".to_string();

        for turn in 1..=MAX_TURNS {
            let user_message = format!(
                "WINDOW LABEL: {window_label}\n\nWINDOW BEING COMPACTED:\n{window_text}\n\nCURRENT DRAFT:\n{draft}\n\nLAST TOOL RESULT:\n{feedback}\n\nTurn {turn}/{MAX_TURNS}. Call exactly one tool.",
                draft = render_draft(&sections),
            );
            let config = SemanticLlmPromptConfig {
                response_json_schema: json!({}),
                temperature: None,
                max_output_tokens: Some(2048),
                reasoning_effort: None,
            };
            let mut request = SemanticLlmRequest::from_config(
                SYSTEM_PROMPT.to_string(),
                vec![SemanticLlmMessage::text("user", user_message)],
                config,
            );
            request.forced_tool_names = Some(tool_names.clone());
            let output = self
                .create_weak_llm()
                .await?
                .generate_tool_calls(request, tools.clone(), None)
                .await?;
            let Some(call) = output.calls.first() else {
                feedback = "no tool call received; call exactly one tool".to_string();
                continue;
            };

            match call.tool_name.as_str() {
                "write_section" => {
                    let section = call
                        .arguments
                        .get("section")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let content = call
                        .arguments
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .unwrap_or_default();
                    match SECTIONS.iter().find(|(name, ..)| *name == section) {
                        Some((name, ..)) if !content.is_empty() => {
                            sections.insert(name, content.to_string());
                            let missing = required_missing(&sections);
                            feedback = if missing.is_empty() {
                                format!("section `{section}` written; all required sections filled — finalize when accurate")
                            } else {
                                format!(
                                    "section `{section}` written; required still empty: {}",
                                    missing.join(", ")
                                )
                            };
                        }
                        Some(_) => feedback = "content must be non-empty".to_string(),
                        None => feedback = format!("unknown section `{section}`"),
                    }
                }
                "search_memories" => {
                    let query = call
                        .arguments
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    feedback = self.summarizer_search_memories(query).await;
                }
                "save_memory" => {
                    let content = call
                        .arguments
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    if content.is_empty() {
                        feedback = "save_memory requires non-empty content".to_string();
                        continue;
                    }
                    let observation = call
                        .arguments
                        .get("observation")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    feedback = self.summarizer_save_memory(content, observation).await;
                }
                "finalize" => {
                    let missing = required_missing(&sections);
                    if missing.is_empty() {
                        return Ok(assemble(&sections));
                    }
                    feedback = format!(
                        "finalize rejected: required section(s) empty: {}",
                        missing.join(", ")
                    );
                }
                other => feedback = format!("unknown tool `{other}`"),
            }
        }

        if required_missing(&sections).is_empty() {
            tracing::warn!(
                thread_id = self.ctx.thread_id,
                "agentic summary hit turn cap without finalize; assembling filled sections"
            );
            return Ok(assemble(&sections));
        }
        Err(AppError::Internal(
            "agentic summary exhausted turns with required sections unfilled".to_string(),
        ))
    }

    async fn summarizer_search_memories(&self, query: String) -> String {
        let thread = match self.ctx.get_thread().await {
            Ok(thread) => thread,
            Err(error) => return format!("memory search unavailable: {error}"),
        };
        let result = commands::LoadAgentMemoryCommand {
            deployment_id: self.ctx.agent.deployment_id,
            agent_id: self.ctx.agent.id,
            thread_id: self.ctx.thread_id,
            actor_id: thread.actor_id,
            project_id: thread.project_id,
            query,
            categories: Vec::new(),
            sources: Vec::new(),
            depth: None,
            search_approach: Default::default(),
        }
        .execute_with_deps(&self.ctx.app_state)
        .await;
        match result {
            Ok(memories) if memories.is_empty() => "no similar memories found".to_string(),
            Ok(memories) => memories
                .iter()
                .take(5)
                .map(|m| {
                    let content: String = m.content.chars().take(200).collect();
                    format!("- [{}] {}", m.memory_category, content)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Err(error) => format!("memory search failed: {error}"),
        }
    }

    async fn summarizer_save_memory(&self, content: String, observation: Option<String>) -> String {
        let thread = match self.ctx.get_thread().await {
            Ok(thread) => thread,
            Err(error) => return format!("memory save unavailable: {error}"),
        };
        let result = commands::SaveAgentMemoryCommand {
            deployment_id: self.ctx.agent.deployment_id,
            agent_id: self.ctx.agent.id,
            thread_id: self.ctx.thread_id,
            execution_run_id: self.ctx.execution_run_id,
            actor_id: thread.actor_id,
            project_id: thread.project_id,
            content,
            category: None,
            scope: None,
            observation,
            signals: Vec::new(),
            related: Vec::new(),
        }
        .execute_with_deps(&self.ctx.app_state)
        .await;
        match result {
            Ok(memory) => {
                crate::executor::context::memory_context::invalidate_startup_memory_cache(
                    &self.ctx.app_state,
                    self.ctx.thread_id,
                )
                .await;
                format!("memory saved (id={})", memory.id)
            }
            Err(error) => format!("memory save failed: {error}"),
        }
    }
}
