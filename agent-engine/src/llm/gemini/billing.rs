use chrono::{Datelike, Utc};

use super::{GeminiClient, GeminiResponse, UsageMetadata};

fn get_model_pricing(model: &str) -> Option<(i64, i64, i64, i64)> {
    match model {
        "gemini-3-flash-preview" => Some((50, 12, 300, 14)),
        "gemini-3.1-pro-preview" => Some((200, 50, 1200, 0)),
        _ => None,
    }
}

impl GeminiClient {
    pub(crate) async fn track_token_usage(&self, usage: &UsageMetadata, response: &GeminiResponse) {
        let Some(deployment_id) = self.deployment_id else {
            return;
        };
        let Some(redis_client) = &self.redis_client else {
            return;
        };

        if let Ok(mut conn) = redis_client.get_multiplexed_async_connection().await {
            let now = Utc::now();
            let period = format!("{}-{:02}", now.year(), now.month());
            let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

            let cached_tokens = usage.cached_content_token_count.unwrap_or(0) as i64;
            let total_prompt_tokens = usage.prompt_token_count as i64;
            let non_cached_input_tokens = total_prompt_tokens.saturating_sub(cached_tokens);
            let output_tokens = usage.candidates_token_count as i64;

            let Some((input_price, cached_price, output_price, search_query_price)) =
                get_model_pricing(&self.model)
            else {
                return;
            };
            let mut search_query_count = 0i64;
            let mut unique_search_queries = std::collections::HashSet::new();
            for candidate in &response.candidates {
                if let Some(grounding) = &candidate.grounding_metadata {
                    if let Some(queries) = &grounding.web_search_queries {
                        for query in queries {
                            let trimmed = query.trim();
                            if !trimmed.is_empty() {
                                search_query_count += 1;
                                unique_search_queries.insert(trimmed.to_lowercase());
                            }
                        }
                    }
                }
            }
            let search_query_unique_count = unique_search_queries.len() as i64;

            let non_cached_cost = (non_cached_input_tokens * input_price) / 1_000_000;
            let cached_cost = (cached_tokens * cached_price) / 1_000_000;
            let input_cost_cents = non_cached_cost + cached_cost;

            let output_cost_cents = (output_tokens * output_price) / 1_000_000;
            let search_query_cost_cents = (search_query_count * search_query_price) / 1_000;

            let webhook_payload = serde_json::json!({
                "model": self.model,
                "is_byok": self.is_byok,
                "thread_id": self.thread_id,
                "input_tokens": total_prompt_tokens,
                "cached_tokens": cached_tokens,
                "output_tokens": output_tokens,
                "input_cost_cents": input_cost_cents,
                "output_cost_cents": output_cost_cents,
                "search_query_count": search_query_count,
                "search_query_unique_count": search_query_unique_count,
                "search_query_cost_cents": search_query_cost_cents,
                "total_cost_cents": input_cost_cents + output_cost_cents + search_query_cost_cents,
                "timestamp": now.to_rfc3339(),
                "prompt_token_count": usage.prompt_token_count,
                "candidates_token_count": usage.candidates_token_count,
                "total_token_count": usage.total_token_count,
                "cached_content_token_count": usage.cached_content_token_count,
                "thoughts_token_count": usage.thoughts_token_count,
            });

            if let Some(nats_client) = &self.nats_client {
                let task_message = dto::json::NatsTaskMessage {
                    task_type: "webhook.event".to_string(),
                    task_id: format!(
                        "model-usage-{}-{}",
                        deployment_id,
                        Utc::now().timestamp_micros()
                    ),
                    payload: serde_json::json!({
                        "deployment_id": deployment_id,
                        "event_type": "agent.model.usage",
                        "event_payload": webhook_payload,
                        "triggered_at": now.to_rfc3339(),
                    }),
                };

                let message_bytes = serde_json::to_vec(&task_message).unwrap_or_default();
                if !message_bytes.is_empty() {
                    let _ = nats_client
                        .publish("worker.tasks.webhook.event", message_bytes.into())
                        .await;
                }
            }

            if !self.is_byok {
                let mut pipe = redis::pipe();
                pipe.atomic()
                    .zincr(
                        &format!("{}:metrics", prefix),
                        "ai_token_input_cost_cents",
                        input_cost_cents,
                    )
                    .ignore()
                    .zincr(
                        &format!("{}:metrics", prefix),
                        "ai_token_output_cost_cents",
                        output_cost_cents,
                    )
                    .ignore()
                    .zincr(
                        &format!("{}:metrics", prefix),
                        "ai_search_queries",
                        search_query_count,
                    )
                    .ignore()
                    .zincr(
                        &format!("{}:metrics", prefix),
                        "ai_search_query_cost_cents",
                        search_query_cost_cents,
                    )
                    .ignore()
                    .expire(&format!("{}:metrics", prefix), 5184000)
                    .ignore()
                    .zincr(
                        &format!("billing:{}:dirty_deployments", period),
                        deployment_id,
                        input_cost_cents + output_cost_cents + search_query_cost_cents,
                    )
                    .ignore()
                    .expire(&format!("billing:{}:dirty_deployments", period), 5184000)
                    .ignore();

                let _: Result<(), redis::RedisError> = pipe.query_async(&mut conn).await;
            }
        }
    }
}
