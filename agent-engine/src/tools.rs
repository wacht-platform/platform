use commands::{Command, GenerateEmbeddingsCommand, SearchKnowledgeBaseEmbeddingsCommand};
use queries::Query;
use common::error::AppError;
use chrono;
use rand;
use base64::Engine;
use common::state::AppState;
use dto::json::{
    ApiToolResult, KnowledgeBaseToolResult, PlatformEventResult, PlatformFunctionData,
    PlatformFunctionResult, StreamEvent, ToolKnowledgeBaseSearchResult,
};
use models::HttpMethod;
use models::{AiTool, AiToolConfiguration, InternalToolType};
use models::{
    ApiToolConfiguration, KnowledgeBaseToolConfiguration, PlatformEventToolConfiguration,
    PlatformFunctionToolConfiguration, InternalToolConfiguration,
};
use serde_json::Value;
use std::collections::HashMap;
use crate::filesystem::{AgentFilesystem, shell::ShellExecutor};
use models::AiAgentWithFeatures;


pub struct ToolExecutor {
    app_state: AppState,
    agent: AiAgentWithFeatures,
    context_id: i64,
    channel: Option<tokio::sync::mpsc::Sender<StreamEvent>>,
}

impl ToolExecutor {
    pub fn new(app_state: AppState, agent: AiAgentWithFeatures, context_id: i64) -> Self {
        Self {
            app_state,
            agent,
            context_id,
            channel: None,
        }
    }

    pub fn with_channel(mut self, channel: tokio::sync::mpsc::Sender<StreamEvent>) -> Self {
        self.channel = Some(channel);
        self
    }

    fn create_lite_llm(&self) -> crate::GeminiClient {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        crate::GeminiClient::new(
            api_key,
            Some("gemini-2.5-flash-lite-preview-06-17".to_string()),
        ).with_billing(self.agent.deployment_id, self.app_state.redis_client.clone())
    }

    pub async fn execute_tool_immediately(
        &self,
        tool: &AiTool,
        execution_params: Value,
        filesystem: &AgentFilesystem,
        shell: &ShellExecutor,
    ) -> Result<Value, AppError> {
        let pipeline: Vec<String> = execution_params
            .get("pipeline")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let result = match &tool.configuration {
            AiToolConfiguration::Api(config) => {
                let result = self
                    .execute_api_tool(tool, config, &execution_params)
                    .await?;
                serde_json::to_value(result)?
            }
            AiToolConfiguration::KnowledgeBase(config) => {
                let result = self
                    .execute_knowledge_base_tool(tool, config, &execution_params)
                    .await?;
                serde_json::to_value(result)?
            }
            AiToolConfiguration::PlatformEvent(config) => {
                let result = self
                    .execute_platform_event_tool(tool, config, &execution_params)
                    .await?;
                serde_json::to_value(result)?
            }
            AiToolConfiguration::PlatformFunction(config) => {
                let result = self
                    .execute_platform_function_tool(tool, config, &execution_params)
                    .await?;
                serde_json::to_value(result)?
            }
            AiToolConfiguration::Internal(config) => {
                self.execute_internal_tool(tool, config, &execution_params, filesystem, shell).await?
            }
        };

        let final_result = if !pipeline.is_empty() {
            let result_str = serde_json::to_string_pretty(&result)?;
            let transformed = shell.apply_pipeline(&result_str, &pipeline).await?;
            serde_json::json!({
                "result": transformed,
                "pipeline_applied": pipeline
            })
        } else {
            result
        };

        let should_truncate = tool.name != "read_file" && tool.name != "read_knowledge_base_documents";

        let result_str = serde_json::to_string_pretty(&final_result)?;
        let char_count = result_str.chars().count();
        let threshold = 2000;

        if should_truncate && char_count > threshold {
            let timestamp = chrono::Utc::now().timestamp_millis();
            let random_suffix: String = (0..4).map(|_| {
                use rand::Rng;
                let idx = rand::thread_rng().gen_range(0..36);
                let chars: Vec<char> = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
                chars[idx]
            }).collect();
            
            let scratch_filename = format!("tool_output_{}_{}.txt", timestamp, random_suffix);
            let scratch_path = format!("scratch/{}", scratch_filename);
            
            let _ = filesystem.write_file(&scratch_path, &result_str, None, None).await;

            let lines = result_str.lines().count();
            let size_bytes = result_str.len();

            let hint = if lines <= 1 && size_bytes > 1000 {
                format!("Output is a large single-line text ({} bytes). Use 'cat {} | jq ...' or 'fold -w 80' to inspect", size_bytes, scratch_path)
            } else {
                format!("Output truncated. Full content in '{}'. Use 'cat', 'grep', or 'read_file' on this path.", scratch_path)
            };

            let preview: String = result_str.chars().take(threshold).collect();

            return Ok(serde_json::json!({
                "preview": preview,
                "truncated": true,
                "original_stats": {
                    "size_bytes": size_bytes,
                    "lines": lines,
                    "char_count": char_count,
                    "saved_to_path": scratch_path
                },
                "hint": hint,
            }));
        }

        Ok(final_result)
    }

    async fn execute_internal_tool(
        &self,
        tool: &AiTool,
        config: &InternalToolConfiguration,
        execution_params: &Value,
        filesystem: &AgentFilesystem,
        shell: &ShellExecutor,
    ) -> Result<Value, AppError> {
        tracing::info!(
            tool_name = %tool.name,
            params = %execution_params,
            "Executing internal tool"
        );
        
        match config.tool_type {
            InternalToolType::ReadFile => {
                let path = execution_params.get("path").and_then(|v| v.as_str());
                tracing::debug!(path = ?path, "ReadFile path extraction");
                let path = path.ok_or_else(|| {
                    tracing::warn!(params = %execution_params, "Path is required but missing");
                    AppError::BadRequest("Path is required".to_string())
                })?;
                let start_line = execution_params.get("start_line").and_then(|v| v.as_u64()).map(|v| v as usize);
                let end_line = execution_params.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize);
                
                let extension = path.split('.').last().unwrap_or("").to_lowercase();
                
                match extension.as_str() {
                    "txt" | "md" | "json" | "yaml" | "yml" | "csv" | "xml" | "html" | "htm" |
                    "js" | "ts" | "jsx" | "tsx" | "py" | "rs" | "go" | "java" | "c" | "cpp" |
                    "h" | "hpp" | "css" | "scss" | "toml" | "ini" | "cfg" | "conf" | "sh" |
                    "bash" | "zsh" | "sql" | "graphql" | "proto" | "env" | "gitignore" | 
                    "dockerfile" | "makefile" | "log" | "" => {
                        let result = filesystem.read_file(path, start_line, end_line).await?;
                        Ok(serde_json::json!({
                            "success": true,
                            "tool": tool.name,
                            "path": path,
                            "file_type": "text",
                            "content": result.content,
                            "total_lines": result.total_lines,
                            "start_line": result.start_line,
                            "end_line": result.end_line
                        }))
                    }
                    
                    "pdf" => {
                        let full_path = filesystem.resolve_path_public(path)?;
                        let cmd = format!("pdftotext \"{}\" -", full_path.display());
                        let output = shell.execute(&cmd).await?;
                        
                        if output.exit_code != 0 {
                            return Ok(serde_json::json!({
                                "success": false,
                                "tool": tool.name,
                                "path": path,
                                "file_type": "pdf",
                                "error": format!("Failed to extract PDF text: {}", output.stderr),
                                "hint": "Ensure pdftotext (poppler-utils) is installed"
                            }));
                        }
                        
                        let content = output.stdout;
                        let lines: Vec<&str> = content.lines().collect();
                        let total_lines = lines.len();
                        
                        let start = start_line.unwrap_or(1).saturating_sub(1);
                        let end = end_line.unwrap_or(total_lines).min(total_lines);
                        let selected: Vec<&str> = lines.iter().skip(start).take(end.saturating_sub(start)).cloned().collect();
                        
                        Ok(serde_json::json!({
                            "success": true,
                            "tool": tool.name,
                            "path": path,
                            "file_type": "pdf",
                            "content": selected.join("\n"),
                            "total_lines": total_lines,
                            "start_line": start + 1,
                            "end_line": end,
                            "note": "Text extracted from PDF via pdftotext"
                        }))
                    }
                    
                    "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" => {
                        let bytes = filesystem.read_file_bytes(path).await?;
                        let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        
                        let mime_type = match extension.as_str() {
                            "jpg" | "jpeg" => "image/jpeg",
                            "png" => "image/png",
                            "webp" => "image/webp",
                            "gif" => "image/gif",
                            "bmp" => "image/bmp",
                            "svg" => "image/svg+xml",
                            _ => "application/octet-stream"
                        };
                        
                        Ok(serde_json::json!({
                            "success": true,
                            "tool": tool.name,
                            "path": path,
                            "file_type": "image",
                            "mime_type": mime_type,
                            "size_bytes": bytes.len(),
                            "base64_data": base64_data,
                            "note": "Image encoded as base64. Can be passed to vision-capable LLM for analysis."
                        }))
                    }
                    
                    _ => {
                        let bytes = filesystem.read_file_bytes(path).await?;
                        Ok(serde_json::json!({
                            "success": true,
                            "tool": tool.name,
                            "path": path,
                            "file_type": "binary",
                            "size_bytes": bytes.len(),
                            "extension": extension,
                            "hint": "Binary file. Cannot display content directly. Consider using a specific tool for this file type."
                        }))
                    }
                }
            }
            InternalToolType::WriteFile => {
                let fs = filesystem;
                let path = execution_params.get("path").and_then(|v| v.as_str())
                    .ok_or(AppError::BadRequest("Path is required".to_string()))?;
                let content = execution_params.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let start_line = execution_params.get("start_line").and_then(|v| v.as_u64()).map(|v| v as usize);
                let end_line = execution_params.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize);
                
                let result = fs.write_file(path, content, start_line, end_line).await?;
                Ok(serde_json::json!({
                    "success": true,
                    "tool": tool.name,
                    "path": path,
                    "lines_written": result.lines_written,
                    "total_lines": result.total_lines,
                    "partial": result.partial
                }))
            }
            InternalToolType::ListDirectory => {
                let fs = filesystem;
                let path = execution_params.get("path").and_then(|v| v.as_str()).unwrap_or("/");
                let files = fs.list_dir(path).await?;
                Ok(serde_json::json!({
                    "success": true,
                    "tool": tool.name,
                    "path": path,
                    "files": files
                }))
            }
            InternalToolType::SearchFiles => {
                let fs = filesystem;
                let query = execution_params.get("query").and_then(|v| v.as_str())
                    .ok_or(AppError::BadRequest("Query is required".to_string()))?;
                let path = execution_params.get("path").and_then(|v| v.as_str()).unwrap_or("/");
                let result = fs.search(query, path).await?;
                Ok(serde_json::json!({
                    "success": true,
                    "tool": tool.name,
                    "path": path,
                    "query": query,
                    "matches": result
                }))
            }
            InternalToolType::ExecuteCommand => {
                let sh = shell;
                let command = execution_params.get("command").and_then(|v| v.as_str())
                    .ok_or(AppError::BadRequest("Command is required".to_string()))?;
                let output = sh.execute(command).await?;
                Ok(serde_json::json!({
                    "success": output.exit_code == 0,
                    "tool": tool.name,
                    "command": command,
                    "stdout": output.stdout,
                    "stderr": output.stderr,
                    "exit_code": output.exit_code
                }))
            }
            InternalToolType::SaveMemory => {
                let content = execution_params.get("content").and_then(|v| v.as_str())
                    .ok_or(AppError::BadRequest("Content is required".to_string()))?;
                let category_str = execution_params.get("category").and_then(|v| v.as_str())
                    .unwrap_or("working");
                let importance = execution_params.get("importance").and_then(|v| v.as_f64())
                    .unwrap_or(0.5);
                
                let category = dto::json::agent_memory::MemoryCategory::from_str(category_str)
                    .unwrap_or(dto::json::agent_memory::MemoryCategory::Working);
                
                let embeddings = commands::GenerateEmbeddingsCommand::new(vec![content.to_string()])
                    .with_task_type("RETRIEVAL_DOCUMENT".to_string())
                    .execute(&self.app_state)
                    .await?;
                
                if embeddings.is_empty() {
                    return Err(AppError::Internal("Failed to generate embedding".to_string()));
                }
                
                let embedding = &embeddings[0];
                
                let similar = queries::FindSimilarMemoriesQuery {
                    agent_id: self.agent.id,
                    embedding: embedding.clone(),
                    threshold: 0.70,
                    limit: 5,
                }.execute(&self.app_state).await?;
                
                let exact_dupe = similar.iter().find(|m| m.similarity > 0.95);
                if let Some(dupe) = exact_dupe {
                    return Ok(serde_json::json!({
                        "success": false,
                        "tool": tool.name,
                        "message": "This information already exists",
                        "existing_content": dupe.content
                    }));
                }
                
                let consolidation_candidates: Vec<_> = similar.iter()
                    .filter(|m| m.similarity >= 0.70 && m.similarity < 0.95)
                    .collect();
                
                let final_content: String;
                let mut consolidated_ids: Vec<i64> = Vec::new();
                let mut _total_access_count: i32 = 0;
                
                if !consolidation_candidates.is_empty() {
                    let existing_facts: Vec<String> = consolidation_candidates.iter()
                        .map(|m| m.content.clone())
                        .collect();
                    
                    let context = serde_json::json!({
                        "new_fact": content,
                        "existing_facts": existing_facts
                    });
                    
                    let request_body = crate::template::render_template_with_prompt(
                        crate::template::AgentTemplates::MEMORY_CONSOLIDATION,
                        context
                    ).map_err(|e| AppError::Internal(format!("Template error: {}", e)))?;
                    
                    let llm = self.create_lite_llm();
                    
                    let (response, _): (dto::json::agent_memory::MemoryConsolidationResponse, _) = llm.generate_structured_content(request_body).await
                        .map_err(|e| AppError::External(format!("LLM consolidation failed: {}", e)))?;
                    
                    if response.decision == "duplicate" {
                        return Ok(serde_json::json!({
                            "success": false,
                            "tool": tool.name,
                            "message": "This information is redundant with existing memories",
                            "reason": response.reasoning
                        }));
                    }
                    
                    final_content = response.consolidated_content.unwrap_or_else(|| content.to_string());
                    
                    for candidate in &consolidation_candidates {
                        consolidated_ids.push(candidate.id);
                    }
                    
                    for id in &consolidated_ids {
                        if let Ok(mem) = (queries::GetMemoryByIdQuery { memory_id: *id })
                            .execute(&self.app_state).await {
                            _total_access_count += mem.access_count;
                        }
                    }
                } else {
                    final_content = content.to_string();
                }
                
                let final_embedding = if final_content != content {
                    let new_embeddings = commands::GenerateEmbeddingsCommand::new(vec![final_content.clone()])
                        .with_task_type("RETRIEVAL_DOCUMENT".to_string())
                        .execute(&self.app_state)
                        .await?;
                    new_embeddings.get(0).cloned().unwrap_or(embedding.clone())
                } else {
                    embedding.clone()
                };
                
                let memory_id = self.app_state.sf.next_id()? as i64;
                let create_cmd = commands::CreateMemoryCommand {
                    id: memory_id,
                    content: final_content.clone(),
                    embedding: final_embedding,
                    memory_category: category.clone(),
                    creation_context_id: Some(self.context_id),
                    agent_id: Some(self.agent.id),
                    initial_importance: importance,
                };
                let memory = create_cmd.execute(&self.app_state).await?;
                
                if !consolidated_ids.is_empty() {
                    commands::DeleteMemoriesCommand {
                        memory_ids: consolidated_ids.clone(),
                    }.execute(&self.app_state).await.ok();
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
                    "consolidated_count": consolidated_count
                }))
            }

            InternalToolType::GenerateIntegrationLink => {
                let integration_type = execution_params
                    .get("integration_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("teams")
                    .to_string();

                // 1. Get the context to find the context_group
                let context = queries::GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
                    .execute(&self.app_state)
                    .await?;

                // 2. Get context_group (subject/audience identifier)
                let context_group = context.context_group
                    .ok_or_else(|| AppError::BadRequest("No context group found (user not identified)".to_string()))?;

                // 3. Generate the code
                let cmd = commands::CreateIntegrationLinkCodeCommand::new(
                    self.agent.deployment_id,
                    context_group,
                    self.agent.id,
                    integration_type.clone(),
                );
                
                let result = cmd.execute(&self.app_state).await?;
                
                // 4. Return the code
                Ok(serde_json::json!({
                    "success": true,
                    "tool": tool.name,
                    "code": result.code,
                    "expires_at": result.expires_at,
                    "integration_type": integration_type,
                    "message": format!(
                        "Generated linking code: {}. It expires at {}. Please send this code to the {} bot to link your account.",
                        result.code, result.expires_at, integration_type
                    )
                }))
            }
        }
    }


    async fn execute_api_tool(
        &self,
        tool: &AiTool,
        config: &ApiToolConfiguration,
        execution_params: &Value,
    ) -> Result<ApiToolResult, AppError> {
        let url_params = execution_params
            .get("url_params")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| {
                        let value_str = match v {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            _ => v.to_string(),
                        };
                        (k.clone(), value_str)
                    })
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default();

        let body = execution_params.get("body").cloned();

        let mut url = config.endpoint.clone();
        let mut query_params = HashMap::new();

        for (key, value) in &url_params {
            let placeholder = format!("{{{key}}}");
            if url.contains(&placeholder) {
                url = url.replace(&placeholder, value);
            } else {
                query_params.insert(key.clone(), value.clone());
            }
        }

        let timeout_secs = config.timeout_seconds.unwrap_or(30);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs as u64))
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build HTTP client: {}", e)))?;

        let mut request_builder = match config.method {
            HttpMethod::GET => client.get(&url),
            HttpMethod::POST => client.post(&url),
            HttpMethod::PUT => client.put(&url),
            HttpMethod::PATCH => client.patch(&url),
            HttpMethod::DELETE => client.delete(&url),
        };

        if let Some(headers) = execution_params.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(header_value) = value.as_str() {
                    request_builder = request_builder.header(key, header_value);
                }
            }
        }

        if !query_params.is_empty() {
            request_builder = request_builder.query(&query_params);
        }

        match config.method {
            HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH => {
                request_builder = request_builder.header("Content-Type", "application/json");
                if let Some(body_value) = body {
                    request_builder = request_builder.json(&body_value);
                } else {
                    request_builder = request_builder.json(&serde_json::json!({}));
                }
            }
            _ => {}
        }

        let response = request_builder.send().await;

        match response {
            Ok(res) => {
                let status = res.status().as_u16();
                let body_text = res.text().await.unwrap_or_default();

                if (200..300).contains(&status) {
                    Ok(ApiToolResult {
                        success: true,
                        status,
                        data: Some(
                            serde_json::from_str::<Value>(&body_text)
                                .unwrap_or(Value::String(body_text)),
                        ),
                        error: None,
                        tool: tool.name.clone(),
                    })
                } else {
                    Ok(ApiToolResult {
                        success: false,
                        status,
                        data: None,
                        error: Some(body_text),
                        tool: tool.name.clone(),
                    })
                }
            }
            Err(e) => Err(AppError::External(format!("API request failed: {e}"))),
        }
    }

    async fn execute_platform_event_tool(
        &self,
        tool: &AiTool,
        config: &PlatformEventToolConfiguration,
        execution_params: &Value,
    ) -> Result<PlatformEventResult, AppError> {
        let event_data = execution_params
            .get("event_data")
            .cloned()
            .or_else(|| config.event_data.clone())
            .unwrap_or(serde_json::json!({}));

        if let Some(channel) = &self.channel {
            let event = StreamEvent::PlatformEvent(config.event_label.clone(), event_data.clone());

            let _ = channel.send(event).await;
        }

        Ok(PlatformEventResult {
            success: true,
            tool: tool.name.clone(),
            event_label: config.event_label.clone(),
            event_data,
            message: "Platform event emitted successfully".to_string(),
        })
    }

    async fn execute_platform_function_tool(
        &self,
        tool: &AiTool,
        config: &PlatformFunctionToolConfiguration,
        execution_params: &Value,
    ) -> Result<PlatformFunctionResult, AppError> {
        let mut function_params = HashMap::new();

        if let Some(schema) = &config.input_schema {
            for field in schema {
                if let Some(value) = execution_params.get(&field.name) {
                    function_params.insert(field.name.clone(), value.clone());
                }
            }
        }

        let execution_id = self.app_state.sf.next_id()? as u64;

        let function_data = PlatformFunctionData {
            execution_id: execution_id.to_string(),
            function_name: config.function_name.clone(),
            parameters: function_params.clone(),
            is_overridable: config.is_overridable,
        };

        if let Some(channel) = &self.channel {
            let event = StreamEvent::PlatformFunction(
                config.function_name.clone(),
                serde_json::to_value(&function_data)?,
            );

            let _ = channel.send(event).await;
        }

        Ok(PlatformFunctionResult {
            success: true,
            tool: tool.name.clone(),
            function: config.function_name.clone(),
            execution_id: execution_id.to_string(),
            status: "pending".to_string(),
        })
    }

    async fn execute_knowledge_base_tool(
        &self,
        tool: &AiTool,
        config: &KnowledgeBaseToolConfiguration,
        execution_params: &Value,
    ) -> Result<KnowledgeBaseToolResult, AppError> {
        let query = execution_params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Internal(
                    "Query parameter is required for knowledge base search".to_string(),
                )
            })?;

        let embeddings_command = GenerateEmbeddingsCommand::new(vec![query.to_string()]);
        let embeddings = embeddings_command.execute(&self.app_state).await?;
        let query_embedding = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("Failed to generate query embedding".to_string()))?;

        let limit = config.search_settings.max_results.unwrap_or(10) as u64;
        let search_command = SearchKnowledgeBaseEmbeddingsCommand::new(
            config.knowledge_base_ids.clone(),
            query_embedding,
            limit,
        );

        let search_results = search_command.execute(&self.app_state).await?;

        let threshold = config.search_settings.similarity_threshold.unwrap_or(0.7);
        let mut all_results: Vec<ToolKnowledgeBaseSearchResult> = search_results
            .into_iter()
            .filter(|result| result.score >= threshold as f64)
            .map(|result| ToolKnowledgeBaseSearchResult {
                content: result.content,
                knowledge_base_id: result.knowledge_base_id.to_string(),
                similarity_score: result.score,
                chunk_index: result.chunk_index,
                document_id: result.document_id.to_string(),
                document_title: result.document_title,
                document_description: result.document_description,
            })
            .collect();

        if config.search_settings.sort_by_relevance {
            all_results.sort_by(|a, b| {
                b.similarity_score
                    .partial_cmp(&a.similarity_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        let max_results = config.search_settings.max_results.unwrap_or(10) as usize;
        all_results.truncate(max_results);

        Ok(KnowledgeBaseToolResult {
            success: true,
            tool: tool.name.clone(),
            query: query.to_string(),
            knowledge_base_ids: config.knowledge_base_ids.clone(),
            results: all_results.clone(),
            total_results: all_results.len(),
            search_settings: serde_json::to_value(&config.search_settings)?,
        })
    }
}
