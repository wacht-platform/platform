use super::{parse_params, ToolExecutor};
use commands::Command;
use common::error::AppError;
use queries::Query;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct SaveMemoryParams {
    content: String,
    category: Option<String>,
    importance: Option<f64>,
}

impl ToolExecutor {
    pub(super) async fn execute_save_memory_tool(
        &self,
        tool: &models::AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: SaveMemoryParams = parse_params(execution_params, "save_memory")?;

        let category_str = params.category.as_deref().unwrap_or("working");
        let importance = params.importance.unwrap_or(0.5);

        let category = dto::json::agent_memory::MemoryCategory::from_str(category_str)
            .unwrap_or(dto::json::agent_memory::MemoryCategory::Working);

        let embeddings = commands::GenerateEmbeddingsCommand::new(vec![params.content.clone()])
            .with_task_type("RETRIEVAL_DOCUMENT".to_string())
            .execute(self.app_state())
            .await?;

        if embeddings.is_empty() {
            return Err(AppError::Internal(
                "Failed to generate embedding".to_string(),
            ));
        }

        let embedding = &embeddings[0];

        let similar = queries::FindSimilarMemoriesQuery {
            agent_id: self.agent().id,
            embedding: embedding.clone(),
            threshold: 0.70,
            limit: 5,
        }
        .execute(self.app_state())
        .await?;

        let exact_dupe = similar.iter().find(|m| m.similarity > 0.95);
        if let Some(dupe) = exact_dupe {
            return Ok(serde_json::json!({
                "success": false,
                "tool": tool.name,
                "message": "This information already exists",
                "existing_content": dupe.content
            }));
        }

        let consolidation_candidates: Vec<_> = similar
            .iter()
            .filter(|m| m.similarity >= 0.70 && m.similarity < 0.95)
            .collect();

        let final_content: String;
        let mut consolidated_ids: Vec<i64> = Vec::new();
        let mut _total_access_count: i32 = 0;

        if !consolidation_candidates.is_empty() {
            let existing_facts: Vec<String> = consolidation_candidates
                .iter()
                .map(|m| m.content.clone())
                .collect();

            let context = serde_json::json!({
                "new_fact": params.content,
                "existing_facts": existing_facts
            });

            let request_body = crate::template::render_template_with_prompt(
                crate::template::AgentTemplates::MEMORY_CONSOLIDATION,
                context,
            )
            .map_err(|e| AppError::Internal(format!("Template error: {}", e)))?;

            let llm = self.create_lite_llm().await;

            let (response, _): (dto::json::agent_memory::MemoryConsolidationResponse, _) = llm
                .generate_structured_content(request_body)
                .await
                .map_err(|e| AppError::External(format!("LLM consolidation failed: {}", e)))?;

            if response.decision == "duplicate" {
                return Ok(serde_json::json!({
                    "success": false,
                    "tool": tool.name,
                    "message": "This information is redundant with existing memories",
                    "reason": response.reasoning
                }));
            }

            final_content = response
                .consolidated_content
                .unwrap_or_else(|| params.content.to_string());

            for candidate in &consolidation_candidates {
                consolidated_ids.push(candidate.id);
            }

            for id in &consolidated_ids {
                if let Ok(mem) = (queries::GetMemoryByIdQuery { memory_id: *id })
                    .execute(self.app_state())
                    .await
                {
                    _total_access_count += mem.access_count;
                }
            }
        } else {
            final_content = params.content.to_string();
        }

        let final_embedding = if final_content != params.content {
            let new_embeddings =
                commands::GenerateEmbeddingsCommand::new(vec![final_content.clone()])
                    .with_task_type("RETRIEVAL_DOCUMENT".to_string())
                    .execute(self.app_state())
                    .await?;
            new_embeddings
                .first()
                .cloned()
                .unwrap_or_else(|| embedding.clone())
        } else {
            embedding.clone()
        };

        let memory_id = self.app_state().sf.next_id()? as i64;
        let create_cmd = commands::CreateMemoryCommand {
            id: memory_id,
            content: final_content.clone(),
            embedding: final_embedding,
            memory_category: category.clone(),
            creation_context_id: Some(self.context_id()),
            agent_id: Some(self.agent().id),
            initial_importance: importance,
        };
        let memory = create_cmd.execute(self.app_state()).await?;

        if !consolidated_ids.is_empty() {
            commands::DeleteMemoriesCommand {
                memory_ids: consolidated_ids.clone(),
            }
            .execute(self.app_state())
            .await
            .ok();
        }

        let consolidated_count = consolidated_ids.len();
        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "message": if consolidated_count > 0 {
                format!("Memory saved (consolidated {} related memories)", consolidated_count)
            } else {
                "Memory saved successfully".to_string()
            },
            "memory_id": memory.id.to_string(),
            "category": category_str,
            "consolidated_count": consolidated_count,
            "created_at": memory.created_at.to_rfc3339(),
            "updated_at": memory.updated_at.to_rfc3339()
        }))
    }
}
