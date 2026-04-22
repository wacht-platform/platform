use crate::llm::{
    GeminiClient, LlmClient, NativeToolDefinition, OpenAiClient, OpenRouterClient, ResolvedLlm,
    SemanticLlmMessage, SemanticLlmRequest,
};
use common::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::json;

const ADMISSION_SYSTEM_PROMPT: &str =
    "You are a connectivity probe. Respond exactly as instructed. Do not add commentary.";

const STRONG_PROBE_PROMPT: &str =
    "Call the `ping` function with argument { \"ok\": true }. Do not output any text.";

const WEAK_PROBE_PROMPT: &str =
    "Respond with a JSON object matching the schema. Set `status` to the literal string `ok`.";

fn build_client(
    provider: &str,
    model: &str,
    api_key: Option<&str>,
    openrouter_require_parameters: bool,
) -> Result<ResolvedLlm, AppError> {
    let api_key = api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Validation(format!("No API key configured for {provider}")))?
        .to_string();
    let model_owned = model.to_string();

    match provider {
        "openai" => {
            let client = OpenAiClient::new(api_key, model_owned.clone());
            Ok(ResolvedLlm::new(LlmClient::OpenAi(client), model_owned))
        }
        "openrouter" => {
            let client =
                OpenRouterClient::new(api_key, model_owned.clone(), openrouter_require_parameters);
            Ok(ResolvedLlm::new(LlmClient::OpenRouter(client), model_owned))
        }
        "gemini" => {
            let client = GeminiClient::new_byok(api_key, model_owned.clone());
            Ok(ResolvedLlm::new(LlmClient::Gemini(client), model_owned))
        }
        other => Err(AppError::Validation(format!(
            "Unsupported LLM provider for admission: {other}"
        ))),
    }
}

pub async fn admit_strong_model(
    provider: &str,
    model: &str,
    api_key: Option<&str>,
    openrouter_require_parameters: bool,
) -> Result<(), AppError> {
    let llm = build_client(provider, model, api_key, openrouter_require_parameters)?;

    let tools = vec![NativeToolDefinition {
        name: "ping".to_string(),
        description: "Connectivity probe. Call with ok=true.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "ok": { "type": "boolean", "description": "Must be true." }
            },
            "required": ["ok"]
        }),
    }];

    let request = SemanticLlmRequest {
        system_prompt: ADMISSION_SYSTEM_PROMPT.to_string(),
        messages: vec![SemanticLlmMessage::text("user", STRONG_PROBE_PROMPT)],
        response_json_schema: json!({}),
        temperature: None,
        max_output_tokens: Some(128),
        reasoning_effort: Some("low".to_string()),
    };

    let output = llm
        .generate_tool_calls(request, tools)
        .await
        .map_err(|e| AppError::Validation(format!("Strong model admission call failed: {e}")))?;

    let call = output.calls.into_iter().next().ok_or_else(|| {
        AppError::Validation(
            "Strong model did not emit a tool call. Pick a model that supports function calling."
                .to_string(),
        )
    })?;

    if call.tool_name != "ping" {
        return Err(AppError::Validation(format!(
            "Strong model called `{}` instead of `ping`. Tool-call routing is unreliable for this model.",
            call.tool_name
        )));
    }

    let ok = call
        .arguments
        .get("ok")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| {
            AppError::Validation(
                "Strong model tool-call arguments did not include a boolean `ok` field."
                    .to_string(),
            )
        })?;

    if !ok {
        return Err(AppError::Validation(
            "Strong model emitted the tool call but did not follow the instruction (ok != true)."
                .to_string(),
        ));
    }

    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct WeakAdmissionResponse {
    status: String,
}

pub async fn admit_weak_model(
    provider: &str,
    model: &str,
    api_key: Option<&str>,
    openrouter_require_parameters: bool,
) -> Result<(), AppError> {
    let llm = build_client(provider, model, api_key, openrouter_require_parameters)?;

    let request = SemanticLlmRequest {
        system_prompt: ADMISSION_SYSTEM_PROMPT.to_string(),
        messages: vec![SemanticLlmMessage::text("user", WEAK_PROBE_PROMPT)],
        response_json_schema: json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" }
            },
            "required": ["status"]
        }),
        temperature: None,
        max_output_tokens: Some(64),
        reasoning_effort: Some("low".to_string()),
    };

    let output = llm
        .generate_structured_from_prompt::<WeakAdmissionResponse>(request, None)
        .await
        .map_err(|e| AppError::Validation(format!("Weak model admission call failed: {e}")))?;

    if output.value.status.trim().to_ascii_lowercase() != "ok" {
        return Err(AppError::Validation(format!(
            "Weak model returned structured output but did not follow the instruction (status=`{}`, expected `ok`).",
            output.value.status
        )));
    }

    Ok(())
}
