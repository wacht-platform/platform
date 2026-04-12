use super::ToolExecutor;
use crate::KnowledgeOrchestrator;
use common::error::AppError;
use dto::json::agent_executor::{LocalKnowledgeSearchType, SearchKnowledgebaseParams};
use serde_json::Value;


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
        let hints = KnowledgeOrchestrator::new(self.ctx.clone())
            .gather_local_knowledge_hints(
                query,
                search_type,
                params.knowledge_base_ids,
                params.max_results.unwrap_or(12) as usize,
            )
            .await?;

        self.serialize_tool_output(hints)
    }
}
