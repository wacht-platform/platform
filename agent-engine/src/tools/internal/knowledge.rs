use super::ToolExecutor;
use crate::KnowledgeOrchestrator;
use common::error::AppError;
use dto::json::agent_executor::{LocalKnowledgeSearchType, SearchKnowledgebaseParams};
use serde_json::Value;

fn truncate_for_log(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}… [{} chars total]", truncated, char_count)
    }
}

impl ToolExecutor {
    pub(super) async fn execute_search_knowledgebase_tool(
        &self,
        params: SearchKnowledgebaseParams,
    ) -> Result<Value, AppError> {
        let query = params.query.trim();
        if query.is_empty() {
            return Err(AppError::BadRequest(
                "search_knowledgebase requires a non-empty `query`".to_string(),
            ));
        }

        let search_type = params
            .search_type
            .unwrap_or(LocalKnowledgeSearchType::Semantic);
        let max_results = params.max_results.unwrap_or(12) as usize;
        let explicit_kb_ids = params.knowledge_base_ids.clone();

        tracing::info!(
            thread_id = self.thread_id(),
            agent_id = self.agent().id,
            query_len = query.chars().count(),
            query_preview = %truncate_for_log(query, 160),
            search_type = ?search_type,
            explicit_kb_ids = ?explicit_kb_ids,
            max_results,
            "search_knowledgebase invoked",
        );

        let started = std::time::Instant::now();
        let hints = KnowledgeOrchestrator::new(self.ctx.clone())
            .gather_local_knowledge_hints(
                query,
                search_type,
                explicit_kb_ids,
                max_results,
            )
            .await?;

        tracing::info!(
            thread_id = self.thread_id(),
            agent_id = self.agent().id,
            elapsed_ms = started.elapsed().as_millis() as u64,
            conclusion = ?hints.search_conclusion,
            recommended_files = hints.recommended_files.len(),
            knowledge_bases_searched = hints.knowledge_bases_searched.len(),
            "search_knowledgebase completed",
        );

        self.serialize_tool_output(hints)
    }
}
