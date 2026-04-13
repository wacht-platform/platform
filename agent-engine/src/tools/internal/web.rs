use super::ToolExecutor;
use commands::DeductPulseCreditsCommand;
use common::error::AppError;
use dto::json::agent_executor::{UrlContentParams, WebSearchParams};
use models::pulse_transaction::PulseTransactionType;
use models::AiTool;
use queries::billing::GetOwnerIdByDeploymentIdQuery;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::warn;

const PARALLEL_API_BASE_URL: &str = "https://api.parallel.ai/v1beta";
const PARALLEL_REQUEST_TIMEOUT_SECS: u64 = 240;
const PARALLEL_TOOL_CHAR_BUDGET: u32 = 200_000;
const MIN_CHAR_LIMIT: u32 = 1_000;
const DEFAULT_WEB_SEARCH_MAX_RESULTS: u32 = 10;
const MAX_WEB_SEARCH_RESULTS: u32 = 50;
const PULSE_MILLI_CENTS_PER_CENT: i64 = 10;
const PARALLEL_SEARCH_COST_MILLI_CENTS: i64 = 5;
const PARALLEL_EXTRACT_COST_MILLI_CENTS: i64 = 1;

#[derive(Debug, Serialize)]
struct SearchRequest {
    mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    objective: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_queries: Option<Vec<String>>,
    max_results: u32,
    excerpts: ExcerptSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_policy: Option<SourcePolicy>,
}

#[derive(Debug, Serialize)]
struct ExtractRequest {
    urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    objective: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_queries: Option<Vec<String>>,
    excerpts: ExtractExcerptsSetting,
    full_content: ExtractFullContentSetting,
}

#[derive(Debug, Serialize)]
struct ExcerptSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_chars_per_result: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_chars_total: Option<u32>,
}

#[derive(Debug, Serialize)]
struct FullContentSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_chars_per_result: Option<u32>,
}

#[derive(Debug, Serialize)]
struct SourcePolicy {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    include_domains: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exclude_domains: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after_date: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ExtractExcerptsSetting {
    Disabled(bool),
    Enabled(ExcerptSettings),
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ExtractFullContentSetting {
    Disabled(bool),
    Enabled(FullContentSettings),
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    search_id: String,
    results: Vec<WebSearchResult>,
    #[serde(default)]
    warnings: Option<Vec<ParallelWarning>>,
    #[serde(default)]
    usage: Option<Vec<ParallelUsageItem>>,
}

#[derive(Debug, Deserialize)]
struct ExtractResponse {
    extract_id: String,
    results: Vec<ExtractResult>,
    errors: Vec<ExtractError>,
    #[serde(default)]
    warnings: Option<Vec<ParallelWarning>>,
    #[serde(default)]
    usage: Option<Vec<ParallelUsageItem>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct WebSearchResult {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    publish_date: Option<String>,
    #[serde(default)]
    excerpts: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ExtractResult {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    publish_date: Option<String>,
    #[serde(default)]
    excerpts: Option<Vec<String>>,
    #[serde(default)]
    full_content: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ExtractError {
    url: String,
    error_type: String,
    #[serde(default)]
    http_status_code: Option<u16>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ParallelWarning {
    #[serde(rename = "type")]
    warning_type: String,
    message: String,
    #[serde(default)]
    detail: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ParallelUsageItem {
    name: String,
    count: i64,
}

fn normalize_char_limit(value: Option<u32>, default: u32) -> u32 {
    value
        .unwrap_or(default)
        .clamp(MIN_CHAR_LIMIT, PARALLEL_TOOL_CHAR_BUDGET)
}

fn non_empty_string(input: Option<String>) -> Option<String> {
    input.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn non_empty_vec(values: Vec<String>) -> Option<Vec<String>> {
    let filtered = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

fn normalize_search_mode(mode: Option<String>) -> Result<String, AppError> {
    match mode.as_deref().unwrap_or("agentic") {
        "agentic" | "one-shot" | "fast" => Ok(mode.unwrap_or_else(|| "agentic".to_string())),
        other => Err(AppError::BadRequest(format!(
            "Invalid web_search mode `{other}`. Expected one of: agentic, one-shot, fast"
        ))),
    }
}

fn compact_error_body(body: &str) -> String {
    const MAX_ERROR_BODY_CHARS: usize = 1_500;
    let trimmed = body.trim();
    if trimmed.chars().count() <= MAX_ERROR_BODY_CHARS {
        trimmed.to_string()
    } else {
        let compact = trimmed
            .chars()
            .take(MAX_ERROR_BODY_CHARS)
            .collect::<String>();
        format!("{compact} ...")
    }
}

impl ToolExecutor {
    pub(super) async fn execute_web_search_tool(
        &self,
        tool: &AiTool,
        params: WebSearchParams,
    ) -> Result<Value, AppError> {
        let objective = non_empty_string(params.objective);
        let search_queries = non_empty_vec(params.search_queries);
        if objective.is_none() && search_queries.is_none() {
            return Err(AppError::BadRequest(
                "web_search requires `objective` or `search_queries`".to_string(),
            ));
        }

        let max_results = params
            .max_results
            .unwrap_or(DEFAULT_WEB_SEARCH_MAX_RESULTS)
            .clamp(1, MAX_WEB_SEARCH_RESULTS);
        let excerpt_total =
            normalize_char_limit(params.excerpt_max_chars_total, PARALLEL_TOOL_CHAR_BUDGET);
        let excerpt_per_result_default =
            (excerpt_total / max_results.max(1)).clamp(MIN_CHAR_LIMIT, 100_000);
        let excerpt_per_result = normalize_char_limit(
            params.excerpt_max_chars_per_result,
            excerpt_per_result_default,
        );

        let source_policy = if params.include_domains.is_empty()
            && params.exclude_domains.is_empty()
            && params.after_date.is_none()
        {
            None
        } else {
            Some(SourcePolicy {
                include_domains: params.include_domains,
                exclude_domains: params.exclude_domains,
                after_date: non_empty_string(params.after_date),
            })
        };

        let request = SearchRequest {
            mode: normalize_search_mode(params.mode)?,
            objective,
            search_queries,
            max_results,
            excerpts: ExcerptSettings {
                max_chars_per_result: Some(excerpt_per_result),
                max_chars_total: Some(excerpt_total),
            },
            source_policy,
        };

        let response: SearchResponse = self
            .post_parallel_json("/search", &request, "search")
            .await?;

        self.charge_parallel_tool_usage(
            PARALLEL_SEARCH_COST_MILLI_CENTS,
            "web_search",
            PulseTransactionType::UsageWebSearch,
            Some(response.search_id.as_str()),
        )
        .await;

        Ok(json!({
            "success": true,
            "tool": tool.name,
            "search_id": response.search_id,
            "result_count": response.results.len(),
            "results": response.results,
            "warnings": response.warnings,
            "usage": response.usage,
            "budget_chars_total": excerpt_total
        }))
    }

    pub(super) async fn execute_url_content_tool(
        &self,
        tool: &AiTool,
        params: UrlContentParams,
    ) -> Result<Value, AppError> {
        let urls = non_empty_vec(params.urls).ok_or_else(|| {
            AppError::BadRequest("url_content requires at least one non-empty URL".to_string())
        })?;

        if !params.excerpts && !params.full_content {
            return Err(AppError::BadRequest(
                "url_content requires `excerpts=true` or `full_content=true`".to_string(),
            ));
        }

        let objective = non_empty_string(params.objective);
        let search_queries = non_empty_vec(params.search_queries);
        let url_count = urls.len().max(1) as u32;
        let excerpt_total =
            normalize_char_limit(params.excerpt_max_chars_total, PARALLEL_TOOL_CHAR_BUDGET);
        let excerpt_per_result_default =
            (excerpt_total / url_count).clamp(MIN_CHAR_LIMIT, PARALLEL_TOOL_CHAR_BUDGET);
        let excerpt_per_result = normalize_char_limit(
            params.excerpt_max_chars_per_result,
            excerpt_per_result_default,
        );
        let full_content_per_result = normalize_char_limit(
            params.full_content_max_chars_per_result,
            (PARALLEL_TOOL_CHAR_BUDGET / url_count).max(MIN_CHAR_LIMIT),
        );

        let request = ExtractRequest {
            urls,
            objective,
            search_queries,
            excerpts: if params.excerpts {
                ExtractExcerptsSetting::Enabled(ExcerptSettings {
                    max_chars_per_result: Some(excerpt_per_result),
                    max_chars_total: Some(excerpt_total),
                })
            } else {
                ExtractExcerptsSetting::Disabled(false)
            },
            full_content: if params.full_content {
                ExtractFullContentSetting::Enabled(FullContentSettings {
                    max_chars_per_result: Some(full_content_per_result),
                })
            } else {
                ExtractFullContentSetting::Disabled(false)
            },
        };

        let response: ExtractResponse = self
            .post_parallel_json("/extract", &request, "extract")
            .await?;

        self.charge_parallel_tool_usage(
            PARALLEL_EXTRACT_COST_MILLI_CENTS,
            "url_content",
            PulseTransactionType::UsageUrlContent,
            Some(response.extract_id.as_str()),
        )
        .await;

        Ok(json!({
            "success": true,
            "partial": !response.errors.is_empty(),
            "tool": tool.name,
            "extract_id": response.extract_id,
            "result_count": response.results.len(),
            "failed_url_count": response.errors.len(),
            "results": response.results,
            "errors": response.errors,
            "warnings": response.warnings,
            "usage": response.usage,
            "budget_chars_total": excerpt_total
        }))
    }

    async fn post_parallel_json<TReq, TResp>(
        &self,
        path: &str,
        request_body: &TReq,
        endpoint_name: &str,
    ) -> Result<TResp, AppError>
    where
        TReq: Serialize + ?Sized,
        TResp: for<'de> Deserialize<'de>,
    {
        let api_key = std::env::var("PARALLEL_API_KEY")
            .map_err(|_| AppError::Internal("PARALLEL_API_KEY is not set".to_string()))?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(PARALLEL_REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build Parallel client: {e}")))?;
        let url = format!("{PARALLEL_API_BASE_URL}{path}");

        let response = client
            .post(&url)
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(request_body)
            .send()
            .await
            .map_err(|e| {
                AppError::External(format!("Parallel {endpoint_name} request failed: {e}"))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            AppError::External(format!(
                "Parallel {endpoint_name} response read failed: {e}"
            ))
        })?;

        if !status.is_success() {
            return Err(AppError::External(format!(
                "Parallel {endpoint_name} API error ({}): {}",
                status,
                compact_error_body(&body)
            )));
        }

        serde_json::from_str::<TResp>(&body).map_err(|e| {
            AppError::External(format!(
                "Failed to parse Parallel {endpoint_name} response: {e}. Body: {}",
                compact_error_body(&body)
            ))
        })
    }

    async fn charge_parallel_tool_usage(
        &self,
        milli_cents: i64,
        tool_name: &str,
        transaction_type: PulseTransactionType,
        request_id: Option<&str>,
    ) {
        if milli_cents <= 0 {
            return;
        }

        let deployment_id = self.agent().deployment_id;
        let owner_id = match GetOwnerIdByDeploymentIdQuery::new(deployment_id)
            .execute_with_db(
                self.app_state()
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await
        {
            Ok(Some(owner_id)) => owner_id,
            Ok(None) => {
                warn!(
                    deployment_id,
                    tool_name, "Skipping Parallel pulse charge because billing owner was not found"
                );
                return;
            }
            Err(error) => {
                warn!(
                    deployment_id,
                    tool_name,
                    error = %error,
                    "Failed to resolve billing owner for Parallel pulse charge"
                );
                return;
            }
        };

        let mut redis = match self
            .app_state()
            .redis_client
            .get_multiplexed_async_connection()
            .await
        {
            Ok(conn) => conn,
            Err(error) => {
                warn!(
                    deployment_id,
                    tool_name,
                    error = %error,
                    "Failed to acquire Redis connection for Parallel pulse charge"
                );
                return;
            }
        };

        let accumulator_key = format!("pulse:parallel:milli_cents:{owner_id}:{tool_name}");
        let updated_total = match redis.incr::<_, _, i64>(&accumulator_key, milli_cents).await {
            Ok(total) => total,
            Err(error) => {
                warn!(
                    deployment_id,
                    tool_name,
                    error = %error,
                    "Failed to increment Parallel pulse accumulator"
                );
                return;
            }
        };

        let cents_to_deduct = updated_total / PULSE_MILLI_CENTS_PER_CENT;
        if cents_to_deduct <= 0 {
            return;
        }

        let remainder = updated_total % PULSE_MILLI_CENTS_PER_CENT;
        let mut pipe = redis::pipe();
        pipe.atomic()
            .set(&accumulator_key, remainder)
            .ignore()
            .expire(&accumulator_key, 60 * 60 * 24 * 90)
            .ignore();

        if let Err(error) = pipe.query_async::<()>(&mut redis).await {
            warn!(
                deployment_id,
                tool_name,
                error = %error,
                "Failed to persist Parallel pulse accumulator remainder"
            );
            return;
        }

        let reference_id = Some(match request_id {
            Some(request_id) => format!("parallel:{tool_name}:{request_id}"),
            None => format!("parallel:{tool_name}"),
        });
        let deps = common::deps::from_app(self.app_state()).db().nats().id();
        if let Err(error) = (DeductPulseCreditsCommand {
            transaction_id: None,
            owner_id,
            amount_pulse_cents: cents_to_deduct,
            transaction_type,
            reference_id,
        })
        .execute_with_deps(&deps)
        .await
        {
            warn!(
                deployment_id,
                tool_name,
                error = %error,
                amount_pulse_cents = cents_to_deduct,
                "Failed to deduct pulse credits for Parallel tool usage"
            );
        }
    }
}
