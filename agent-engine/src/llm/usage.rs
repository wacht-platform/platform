use chrono::Utc;

use super::UsageMetadata;

#[derive(Default)]
pub(crate) struct ModelUsageContext<'a> {
    pub deployment_id: Option<i64>,
    pub thread_id: Option<i64>,
    pub actor_id: Option<i64>,
    pub model: &'a str,
    pub is_byok: bool,
    pub nats_client: Option<&'a async_nats::Client>,
    pub search_queries: &'a [String],
}

pub(crate) async fn publish_model_usage(ctx: ModelUsageContext<'_>, usage: &UsageMetadata) {
    let Some(deployment_id) = ctx.deployment_id else {
        return;
    };
    let Some(nats_client) = ctx.nats_client else {
        return;
    };

    let now = Utc::now();
    let cached_tokens = usage.cached_content_token_count.unwrap_or(0) as i64;
    let total_prompt_tokens = usage.prompt_token_count as i64;
    let output_tokens = usage.candidates_token_count as i64;

    let mut search_query_count = 0i64;
    let mut unique_search_queries = std::collections::HashSet::new();
    for query in ctx.search_queries {
        let trimmed = query.trim();
        if !trimmed.is_empty() {
            search_query_count += 1;
            unique_search_queries.insert(trimmed.to_lowercase());
        }
    }
    let search_query_unique_count = unique_search_queries.len() as i64;

    let webhook_payload = serde_json::json!({
        "model": ctx.model,
        "is_byok": ctx.is_byok,
        "thread_id": ctx.thread_id.map(|id| id.to_string()),
        "actor_id": ctx.actor_id.map(|id| id.to_string()),
        "input_tokens": total_prompt_tokens,
        "cached_tokens": cached_tokens,
        "output_tokens": output_tokens,
        "search_query_count": search_query_count,
        "search_query_unique_count": search_query_unique_count,
        "timestamp": now.to_rfc3339(),
        "prompt_token_count": usage.prompt_token_count,
        "candidates_token_count": usage.candidates_token_count,
        "total_token_count": usage.total_token_count,
        "cached_content_token_count": usage.cached_content_token_count,
        "thoughts_token_count": usage.thoughts_token_count,
    });

    let task_message = dto::json::NatsTaskMessage {
        task_type: "webhook.event".to_string(),
        task_id: format!("model-usage-{}-{}", deployment_id, now.timestamp_micros()),
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
