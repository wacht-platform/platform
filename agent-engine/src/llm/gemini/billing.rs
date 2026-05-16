use super::{GeminiClient, GeminiResponse, UsageMetadata};
use crate::llm::usage::{publish_model_usage, ModelUsageContext};

impl GeminiClient {
    pub(crate) async fn track_token_usage(&self, usage: &UsageMetadata, response: &GeminiResponse) {
        let mut search_queries: Vec<String> = Vec::new();
        for candidate in &response.candidates {
            if let Some(grounding) = &candidate.grounding_metadata {
                if let Some(queries) = &grounding.web_search_queries {
                    search_queries.extend(queries.iter().cloned());
                }
            }
        }

        publish_model_usage(
            ModelUsageContext {
                deployment_id: self.deployment_id,
                thread_id: self.thread_id,
                actor_id: self.actor_id,
                model: &self.model,
                is_byok: self.is_byok,
                nats_client: self.nats_client.as_ref(),
                search_queries: &search_queries,
            },
            usage,
        )
        .await;
    }
}
