use crate::executor::python::PythonExecutor;
use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use crate::teams_logger::TeamsActivityLogger;
use base64::Engine;
use chrono;
use commands::{Command, GenerateEmbeddingsCommand, SearchKnowledgeBaseEmbeddingsCommand};
use common::error::AppError;
use common::state::AppState;
use dto::json::{
    ApiToolResult, KnowledgeBaseToolResult, PlatformEventResult, PlatformFunctionData,
    PlatformFunctionResult, StreamEvent, ToolKnowledgeBaseSearchResult,
};
use flate2::read::GzDecoder;
use models::AiAgentWithFeatures;
use models::HttpMethod;
use models::{AiTool, AiToolConfiguration, InternalToolType, UseExternalServiceToolType};
use models::{
    ApiToolConfiguration, InternalToolConfiguration, KnowledgeBaseToolConfiguration,
    PlatformEventToolConfiguration, PlatformFunctionToolConfiguration,
    UseExternalServiceToolConfiguration,
};
use queries::Query;
use rand;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;

pub struct ToolExecutor {
    ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
    channel: Option<tokio::sync::mpsc::Sender<StreamEvent>>,
}

impl ToolExecutor {
    pub fn new(ctx: std::sync::Arc<crate::execution_context::ExecutionContext>) -> Self {
        Self { ctx, channel: None }
    }

    pub fn with_channel(mut self, channel: tokio::sync::mpsc::Sender<StreamEvent>) -> Self {
        self.channel = Some(channel);
        self
    }

    // Accessor methods for backward compatibility
    #[inline]
    fn app_state(&self) -> &AppState {
        &self.ctx.app_state
    }

    #[inline]
    fn agent(&self) -> &AiAgentWithFeatures {
        &self.ctx.agent
    }

    #[inline]
    fn context_id(&self) -> i64 {
        self.ctx.context_id
    }

    async fn create_lite_llm(&self) -> crate::GeminiClient {
        self.ctx.create_llm("gemini-2.5-flash-lite").await.unwrap_or_else(|_| {
            let api_key = std::env::var("GEMINI_API_KEY").unwrap();
            crate::GeminiClient::new(api_key, "gemini-2.5-flash-lite".to_string())
                .with_billing(self.agent().deployment_id, self.app_state().redis_client.clone())
                .with_nats(self.app_state().nats_client.clone())
        })
    }

    pub async fn execute_tool_immediately(
        &self,
        tool: &AiTool,
        execution_params: Value,
        filesystem: &AgentFilesystem,
        shell: &ShellExecutor,
        context_title: &str,
    ) -> Result<Value, AppError> {
        let pipeline: Vec<String> = execution_params
            .get("pipeline")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
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
                self.execute_internal_tool(tool, config, &execution_params, filesystem, shell)
                    .await?
            }
            AiToolConfiguration::UseExternalService(config) => {
                self.execute_external_service_tool(tool, config, &execution_params, context_title, filesystem)
                    .await?
            }
        };

        let mut result = result;
        if result.is_object() && result.get("structure_hint").is_none() {
            let mut special_hint = None;
            for key in ["data", "result", "stdout"] {
                if let Some(val) = result.get(key) {
                    if let Some(s) = val.as_str() {
                        if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                            special_hint = Some(format!("(key '{}' contains parsed JSON) {}", key, infer_schema_hint(&parsed)));
                            break;
                        }
                    }
                }
            }
            
            let hint = special_hint.unwrap_or_else(|| infer_schema_hint(&result));
            
            if let Some(obj) = result.as_object_mut() {
                obj.insert("structure_hint".to_string(), serde_json::json!(hint));
            }
        }

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

        let mut final_result = final_result;
        if tool.name == "read_file" {
            if let Some(content) = final_result.get("content").and_then(|c| c.as_str()) {
                if content.len() > 12000 {
                    let truncated = format!("{}... \n[TRUNCATED: Content too long. Use start_line/end_line to read more]", &content[..2000]);
                    if let Some(obj) = final_result.as_object_mut() {
                        obj.insert("content".to_string(), serde_json::Value::String(truncated));
                        obj.insert("truncated".to_string(), serde_json::Value::Bool(true));
                    }
                }
            }
        }

        let should_truncate =
            tool.name != "read_file" && tool.name != "read_knowledge_base_documents" && tool.name != "teams_analyze_meeting";

        let result_str = serde_json::to_string_pretty(&final_result)?;
        let char_count = result_str.chars().count();
        let threshold = 2400;

        if should_truncate && char_count > threshold {
            let timestamp = chrono::Utc::now().timestamp_millis();
            let random_suffix: String = (0..4)
                .map(|_| {
                    use rand::Rng;
                    let idx = rand::thread_rng().gen_range(0..36);
                    let chars: Vec<char> = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
                    chars[idx]
                })
                .collect();

            let scratch_filename = format!("tool_output_{}_{}.txt", timestamp, random_suffix);
            let scratch_path = format!("scratch/{}", scratch_filename);

            let _ = filesystem
                .write_file(&scratch_path, &result_str, None, None)
                .await;

            let lines = result_str.lines().count();
            let size_bytes = result_str.len();

            let hint = if lines <= 1 && size_bytes > 1000 {
                format!("Output is a large single-line text ({} bytes). Use 'cat {} | jq ...' or 'fold -w 80' to inspect", size_bytes, scratch_path)
            } else {
                format!("Output truncated. Full content in '{}'. Use 'cat', 'grep', or 'read_file' on this path.", scratch_path)
            };

            let preview: String = result_str.chars().take(threshold).collect();

            // Extract the already-computed structure_hint from final_result
            let structure_hint = final_result
                .get("structure_hint")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            return Ok(serde_json::json!({
                "preview": preview,
                "truncated": true,
                "structure_hint": structure_hint,
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
                let start_line = execution_params
                    .get("start_line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let end_line = execution_params
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);

                let extension = path.split('.').last().unwrap_or("").to_lowercase();

                match extension.as_str() {
                    "txt" | "md" | "json" | "yaml" | "yml" | "csv" | "xml" | "html" | "htm"
                    | "js" | "ts" | "jsx" | "tsx" | "py" | "rs" | "go" | "java" | "c" | "cpp"
                    | "h" | "hpp" | "css" | "scss" | "toml" | "ini" | "cfg" | "conf" | "sh"
                    | "bash" | "zsh" | "sql" | "graphql" | "proto" | "env" | "gitignore"
                    | "dockerfile" | "makefile" | "log" | "" => {
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
                        let selected: Vec<&str> = lines
                            .iter()
                            .skip(start)
                            .take(end.saturating_sub(start))
                            .cloned()
                            .collect();

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

                        let mime_type = match extension.as_str() {
                            "jpg" | "jpeg" => "image/jpeg",
                            "png" => "image/png",
                            "webp" => "image/webp",
                            "gif" => "image/gif",
                            "bmp" => "image/bmp",
                            "svg" => "image/svg+xml",
                            _ => "application/octet-stream",
                        };

                        Ok(serde_json::json!({
                            "success": true,
                            "tool": tool.name,
                            "path": path,
                            "file_type": "image",
                            "mime_type": mime_type,
                            "size_bytes": bytes.len(),
                            "note": "This is an image file. To visually analyze its contents, ask the user to re-upload or re-attach this image in their next message. Reading an image file only provides metadata, not visual analysis capability."
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
                let path = execution_params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or(AppError::BadRequest("Path is required".to_string()))?;
                let content = execution_params
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let start_line = execution_params
                    .get("start_line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let end_line = execution_params
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);

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
                let path = execution_params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/");
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
                let query = execution_params
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or(AppError::BadRequest("Query is required".to_string()))?;
                let path = execution_params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/");
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
                let command = execution_params
                    .get("command")
                    .and_then(|v| v.as_str())
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
            InternalToolType::ExecutePython => {
                let script_path_str = execution_params
                    .get("script_path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AppError::BadRequest("Missing script_path".to_string()))?;

                let args_str = execution_params
                    .get("args")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let args: Vec<String> = args_str.split_whitespace().map(String::from).collect();

                let execution_root = filesystem.execution_root();
                let script_path = std::path::Path::new(script_path_str);

                let executor = crate::executor::python::NsJailExecutor::new();

                let result = executor
                    .execute_script(
                        &execution_root,
                        script_path,
                        args,
                        30, // Default timeout 30s
                    )
                    .await?;

                Ok(serde_json::to_value(result)?)
            }
            InternalToolType::SaveMemory => {
                let content = execution_params
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or(AppError::BadRequest("Content is required".to_string()))?;
                let category_str = execution_params
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("working");
                let importance = execution_params
                    .get("importance")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5);

                let category = dto::json::agent_memory::MemoryCategory::from_str(category_str)
                    .unwrap_or(dto::json::agent_memory::MemoryCategory::Working);

                let embeddings =
                    commands::GenerateEmbeddingsCommand::new(vec![content.to_string()])
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
                        "new_fact": content,
                        "existing_facts": existing_facts
                    });

                    let request_body = crate::template::render_template_with_prompt(
                        crate::template::AgentTemplates::MEMORY_CONSOLIDATION,
                        context,
                    )
                    .map_err(|e| AppError::Internal(format!("Template error: {}", e)))?;

                    let llm = self.create_lite_llm().await;

                    let (response, _): (dto::json::agent_memory::MemoryConsolidationResponse, _) =
                        llm.generate_structured_content(request_body)
                            .await
                            .map_err(|e| {
                                AppError::External(format!("LLM consolidation failed: {}", e))
                            })?;

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
                        .unwrap_or_else(|| content.to_string());

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
                    final_content = content.to_string();
                }

                let final_embedding = if final_content != content {
                    let new_embeddings =
                        commands::GenerateEmbeddingsCommand::new(vec![final_content.clone()])
                            .with_task_type("RETRIEVAL_DOCUMENT".to_string())
                            .execute(self.app_state())
                            .await?;
                    new_embeddings.get(0).cloned().unwrap_or(embedding.clone())
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
                    "consolidated_count": consolidated_count
                }))
            }
        }
    }

    async fn execute_external_service_tool(
        &self,
        tool: &AiTool,
        config: &UseExternalServiceToolConfiguration,
        execution_params: &Value,
        context_title: &str,
        filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        match config.service_type {
            UseExternalServiceToolType::TeamsListUsers => {
                self.execute_teams_command(tool, "list_users", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::TeamsSearchUsers => {
                self.execute_teams_command(tool, "search_users", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::TeamsSendDm => {
                self.execute_teams_command(tool, "send_dm", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::TeamsSendContextMessage => {
                self.execute_teams_command(
                    tool,
                    "send_context_message",
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::TeamsListMessages => {
                self.execute_teams_command(tool, "list_messages", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::TeamsGetMeetingRecording => {
                self.execute_teams_command(
                    tool,
                    "get_meeting_recording",
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::TeamsTranscribeMeeting => {
                self.execute_teams_command(tool, "analyze_meeting", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::TeamsSaveAttachment => {
                self.execute_teams_save_attachment(tool, execution_params)
                    .await
            }
            UseExternalServiceToolType::TeamsDescribeImage => {
                self.execute_teams_command(tool, "describe_image", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::TeamsTranscribeAudio => {
                self.execute_teams_command(
                    tool,
                    "transcribe_audio",
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::TeamsListContexts => {
                self.execute_teams_list_contexts(execution_params).await
            }
            UseExternalServiceToolType::TriggerContext => {
                self.execute_trigger_context(tool, execution_params).await
            }
            UseExternalServiceToolType::ClickUpCreateTask => {
                self.execute_clickup_command(tool, "create_task", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpCreateList => {
                self.execute_clickup_command(tool, "create_list", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpUpdateTask => {
                self.execute_clickup_command(tool, "update_task", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpAddComment => {
                self.execute_clickup_command(tool, "add_comment", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetTask => {
                self.execute_clickup_command(tool, "get_task", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetSpaceLists => {
                self.execute_clickup_command(
                    tool,
                    "get_space_lists",
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::ClickUpGetSpaces => {
                self.execute_clickup_command(tool, "get_spaces", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetTeams => {
                self.execute_clickup_command(tool, "get_teams", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetCurrentUser => {
                self.execute_clickup_command(
                    tool,
                    "get_current_user",
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::ClickUpGetTasks => {
                self.execute_clickup_command(tool, "get_tasks", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpSearchTasks => {
                self.execute_clickup_command(tool, "search_tasks", execution_params, context_title)
                    .await
            }
            UseExternalServiceToolType::ClickUpTaskAddAttachment => {
                self.execute_clickup_add_attachment(tool, execution_params, filesystem)
                    .await
            }
        }
    }

    async fn execute_clickup_command(
        &self,
        tool: &AiTool,
        action: &str,
        execution_params: &Value,
        _context_title: &str,
    ) -> Result<Value, AppError> {
        let client = self.ctx.get_clickup_client().await?;

        // Execute action
        let result = match action {
            "get_current_user" => client.get_current_user().await?,
            "get_teams" => client.get_teams().await?,
            "get_spaces" => {
                let team_id = execution_params
                    .get("team_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("team_id is required".to_string()))?;
                client.get_spaces(team_id, execution_params).await?
            }
            "get_space_lists" => {
                let space_id = execution_params
                    .get("space_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("space_id is required".to_string()))?;
                client.get_space_lists(space_id).await?
            }
            "get_task" => {
                let task_id = execution_params
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("task_id is required".to_string()))?;
                client.get_task(task_id).await?
            }
            "get_tasks" => {
                let list_id = execution_params
                    .get("list_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("list_id is required".to_string()))?;
                client.get_tasks(list_id, execution_params).await?
            }
            "search_tasks" => {
                let team_id = execution_params
                    .get("team_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("team_id is required".to_string()))?;
                client.search_tasks(team_id, execution_params).await?
            }
            "create_task" => {
                let list_id = execution_params
                    .get("list_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("list_id is required".to_string()))?;
                client.create_task(list_id, execution_params).await?
            }
            "create_list" => {
                let space_id = execution_params
                    .get("space_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("space_id is required".to_string()))?;
                client.create_list(space_id, execution_params).await?
            }
            "update_task" => {
                let task_id = execution_params
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("task_id is required".to_string()))?;
                client.update_task(task_id, execution_params).await?
            }
            "add_comment" => {
                let task_id = execution_params
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("task_id is required".to_string()))?;
                client.add_comment(task_id, execution_params).await?
            }
            "add_attachment" => {
                let task_id = execution_params
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .ok_or_else(|| AppError::BadRequest("task_id is required".to_string()))?;
                let filename = execution_params
                    .get("filename")
                    .and_then(|v| v.as_str())
                    .unwrap_or("attachment");
                let mime_type = execution_params
                    .get("mime_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream");
                let file_base64 = execution_params
                    .get("file_data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AppError::BadRequest("file_data is required".to_string()))?;
                
                let file_data = base64::engine::general_purpose::STANDARD
                    .decode(file_base64)
                    .map_err(|e| AppError::BadRequest(format!("Invalid base64 file data: {}", e)))?;
                
                client.add_attachment(task_id, filename, mime_type, file_data).await?
            }
            _ => {
                return Err(AppError::BadRequest(format!("Unknown ClickUp action: {}", action)));
            }
        };

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "result": result
        }))
    }

    async fn execute_clickup_add_attachment(
        &self,
        tool: &AiTool,
        execution_params: &Value,
        filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        let task_id = execution_params
            .get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("task_id is required".to_string()))?;

        let file_path = execution_params
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("file_path is required".to_string()))?;

        // Read the file from the agent filesystem
        let file_bytes = filesystem.read_file_bytes(file_path).await?;
        
        // Infer filename from path
        let filename = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment");

        // Infer mime type from extension
        let extension = std::path::Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        let mime_type = match extension.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "pdf" => "application/pdf",
            "txt" => "text/plain",
            "csv" => "text/csv",
            "json" => "application/json",
            "xml" => "application/xml",
            "zip" => "application/zip",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            _ => "application/octet-stream",
        };

        // Encode file as base64 for NATS transport
        let file_base64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
        let file_size = file_bytes.len();

        // Build params with file data and reuse execute_clickup_command
        let enhanced_params = serde_json::json!({
            "task_id": task_id,
            "filename": filename,
            "mime_type": mime_type,
            "file_data": file_base64
        });

        let mut result = self
            .execute_clickup_command(tool, "add_attachment", &enhanced_params, "")
            .await?;

        // Add file info to the response
        if let Some(obj) = result.as_object_mut() {
            obj.insert("uploaded_file".to_string(), serde_json::json!(filename));
            obj.insert("file_size_bytes".to_string(), serde_json::json!(file_size));
        }

        Ok(result)
    }


    async fn execute_teams_command(
        &self,
        tool: &AiTool,
        action: &str,
        execution_params: &Value,
        context_title: &str,
    ) -> Result<Value, AppError> {
        // Check if a target_context_id is provided for cross-context operations
        let target_context_id = execution_params.get("context_id").and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        });

        // Get the context - use cached context for current, query for cross-context
        let (context, effective_context_id) = if let Some(target_id) = target_context_id {
            let ctx = self.ctx.get_context_by_id(target_id)
                .await
                .map_err(|_| {
                    AppError::BadRequest(format!(
                        "Context {} not found or not accessible",
                        target_id
                    ))
                })?;
            (ctx, target_id)
        } else {
            (self.ctx.get_context().await?, self.ctx.context_id)
        };

        // Validate that target context is a Teams context if cross-context
        if target_context_id.is_some() {
            if context.source.as_deref() != Some("teams") {
                return Err(AppError::BadRequest(
                    "Target context is not a Teams context".to_string(),
                ));
            }
        }

        let context_group = context.context_group.ok_or_else(|| {
            AppError::BadRequest("No context group found for Teams command".to_string())
        })?;

        let mut params = execution_params.clone();
        if action == "send_dm" {
            if let Some(obj) = params.as_object_mut() {
                obj.insert(
                    "source_context_id".to_string(),
                    serde_json::json!(self.ctx.context_id.to_string()),
                );
            }
        }

        let payload = serde_json::json!({
            "deployment_id": self.ctx.agent.deployment_id.to_string(),
            "context_id": effective_context_id.to_string(),
            "context_group": context_group,
            "agent_id": self.ctx.agent.id.to_string(),
            "action": action,
            "params": params
        });

        let subject = "integrations.teams.command";

        // NATS client has 5-minute request_timeout configured globally in common/state.rs
        let response = self
            .app_state()
            .nats_client
            .request(subject.to_string(), serde_json::to_vec(&payload)?.into())
            .await
            .map_err(|e| AppError::External(format!("Teams integration request failed: {}", e)))?;

        let payload = response.payload.clone();
        let is_gzipped = payload.len() > 2 && payload[0] == 0x1f && payload[1] == 0x8b;

        let response_data: Value = if is_gzipped {
            let mut decoder = GzDecoder::new(&payload[..]);
            let mut decoded_string = String::new();
            decoder
                .read_to_string(&mut decoded_string)
                .map_err(|e| AppError::External(format!("Decompression failed: {}", e)))?;
            serde_json::from_str(&decoded_string)?
        } else {
            serde_json::from_slice(&payload)?
        };

        // Check if the response indicates an error from the integration service
        if response_data.get("success") == Some(&serde_json::json!(false)) {
            let error_msg = response_data
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error from Teams integration");
            return Ok(serde_json::json!({
                "success": false,
                "tool": tool.name,
                "error": error_msg
            }));
        }

        // Log success
        let logger = TeamsActivityLogger::new(
            &self.agent().deployment_id.to_string(),
            &self.agent().id.to_string(),
            &context_group,
            context_title,
        );
        let _ = logger.ensure_directory().await;

        match action {
            "send_dm" => {
                let user_id = execution_params
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let message = execution_params
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let message_preview = if message.len() > 50 {
                    format!("{}...", &message[..50])
                } else {
                    message.to_string()
                };
                let _ = logger
                    .append_entry(
                        "DM_SENT",
                        &format!("to user {} -> Message: '{}'", user_id, message_preview),
                    )
                    .await;
            }
            "search_users" => {
                let query = execution_params
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // Determine results count for simple logging
                let count = response_data
                    .get("users")
                    .and_then(|u| u.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);

                let _ = logger
                    .append_entry(
                        "SEARCH",
                        &format!("query='{}' -> Found {} users", query, count),
                    )
                    .await;
            }
            "list_users" => {
                let count = response_data
                    .get("users")
                    .and_then(|u| u.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let _ = logger
                    .append_entry("LIST_USERS", &format!("Listed {} users", count))
                    .await;
            }
            _ => {}
        }
        
        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "result": response_data
        }))
    }

    async fn execute_teams_save_attachment(
        &self,
        tool: &AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let attachment_url = execution_params
            .get("attachment_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("attachment_url is required".to_string()))?;

        let filename = execution_params
            .get("filename")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("filename is required".to_string()))?;

        // Get context group for NATS routing
        let context = self.ctx.get_context().await?;

        let context_group = context
            .context_group
            .ok_or_else(|| AppError::BadRequest("No context group found".to_string()))?;

        // Request worker to download the image and return base64 data
        let payload = serde_json::json!({
            "deployment_id": self.agent().deployment_id.to_string(),
            "context_id": self.context_id().to_string(),
            "context_group": context_group,
            "agent_id": self.agent().id.to_string(),
            "action": "download_attachment",
            "params": { "attachment_url": attachment_url }
        });

        let response = self
            .app_state()
            .nats_client
            .request(
                "integrations.teams.command".to_string(),
                serde_json::to_vec(&payload)?.into(),
            )
            .await
            .map_err(|e| AppError::External(format!("Failed to download attachment: {}", e)))?;

        let response_data: Value = serde_json::from_slice(&response.payload)?;

        if response_data.get("success") != Some(&serde_json::json!(true)) {
            let error_msg = response_data
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Failed to download attachment");
            return Ok(serde_json::json!({
                "success": false,
                "tool": tool.name,
                "error": error_msg
            }));
        }

        // Get base64 data from response
        let base64_data = response_data
            .get("data")
            .and_then(|d| d.as_str())
            .ok_or_else(|| AppError::Internal("No data in download response".to_string()))?;

        // Decode base64 data
        use base64::{engine::general_purpose::STANDARD, Engine};
        let bytes = STANDARD
            .decode(base64_data)
            .map_err(|e| AppError::Internal(format!("Invalid base64 data: {}", e)))?;

        // Create filesystem instance for saving
        let execution_id = self
            .app_state()
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate ID: {}", e)))?
            .to_string();

        let filesystem = AgentFilesystem::new(
            &self.agent().deployment_id.to_string(),
            &self.agent().id.to_string(),
            &self.context_id().to_string(),
            &execution_id,
        );

        let clean_filename = std::path::Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment");

        let saved_path = filesystem.save_upload(clean_filename, &bytes).await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "result": {
                "saved": true,
                "path": saved_path,
                "description": response_data.get("description")
            }
        }))
    }

    async fn execute_teams_list_contexts(
        &self,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let limit = execution_params
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(25) as u32;

        let offset = execution_params
            .get("offset")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as u32;

        // Get current context to find the context_group
        let current_context = self.ctx.get_context().await?;

        let context_group = current_context.context_group.ok_or_else(|| {
            AppError::BadRequest(
                "No context group found - this tool requires a Teams context".to_string(),
            )
        })?;

        // Query all Teams contexts in the same context_group
        let contexts = queries::ListExecutionContextsQuery::new(self.ctx.agent.deployment_id)
            .with_source_filter("teams".to_string())
            .with_context_group_filter(context_group.clone())
            .with_limit(limit)
            .with_offset(offset)
            .execute(&self.ctx.app_state)
            .await?;

        let result: Vec<serde_json::Value> = contexts
            .iter()
            .map(|ctx| {
                serde_json::json!({
                    "context_id": ctx.id.to_string(),
                    "title": ctx.title,
                    "status": ctx.status.to_string(),
                    "last_activity": ctx.last_activity_at.to_rfc3339(),
                    "is_current": ctx.id == self.context_id()
                })
            })
            .collect();

        Ok(serde_json::json!({
            "contexts": result,
            "total": result.len(),
            "offset": offset,
            "context_group": context_group,
            "hint": "Use trigger_context with a context_id to send a message to another channel/chat"
        }))
    }

    async fn execute_trigger_context(
        &self,
        tool: &AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let target_context_id = execution_params
            .get("target_context_id")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
            .ok_or_else(|| AppError::BadRequest("target_context_id is required".to_string()))?;

        let message = execution_params
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("message is required".to_string()))?;

        let actionable_id = execution_params
            .get("actionable_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let trigger_execution = execution_params
            .get("execute")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let target_context = self.ctx.get_context_by_id(target_context_id)
            .await
            .map_err(|_| {
                AppError::BadRequest(format!(
                    "Target context {} not found or not accessible",
                    target_context_id
                ))
            })?;

        let conversation_id = self.ctx.app_state.sf
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate ID: {}", e)))?
            as i64;
        let relayed_message = format!(
            "[Cross-context message from context #{}] {}{}",
            self.context_id(),
            message,
            actionable_id
                .as_ref()
                .map(|id| format!(" (actionable: {})", id))
                .unwrap_or_default()
        );

        let content = models::ConversationContent::UserMessage {
            message: relayed_message.clone(),
            sender_name: Some(format!("Cross-context relay from #{}", self.context_id())),
            files: None,
        };

        let conversation_cmd = commands::CreateConversationCommand::new(
            conversation_id,
            target_context_id,
            content,
            models::ConversationMessageType::UserMessage,
        );
        conversation_cmd.execute(self.app_state()).await?;

        if trigger_execution {
            let exec_cmd = commands::PublishAgentExecutionCommand::new_message(
                self.agent().deployment_id,
                target_context_id,
                Some(self.agent().id),
                None,
                conversation_id,
            );

            if let Err(e) = exec_cmd.execute(self.app_state()).await {
                tracing::error!(
                    target_context_id = target_context_id,
                    error = %e,
                    "Failed to trigger cross-context execution"
                );
            } else {
                tracing::info!(
                    target_context_id = target_context_id,
                    "Cross-context execution triggered successfully"
                );
            }
        }

        // Clear the fulfilled actionable from the current context
        if let Some(ref fulfilled_id) = actionable_id {
            let current_context = self.ctx.get_context().await?;

            if let Some(mut metadata) = current_context.external_resource_metadata {
                if let Some(actionables) = metadata.get_mut("actionables") {
                    if let Some(arr) = actionables.as_array_mut() {
                        arr.retain(|a| {
                            a.get("id").and_then(|id| id.as_str()) != Some(fulfilled_id.as_str())
                        });

                        // Update the context with cleaned actionables
                        commands::UpdateExecutionContextQuery::new(
                            self.ctx.context_id,
                            self.ctx.agent.deployment_id,
                        )
                        .with_external_resource_metadata(metadata)
                        .execute(&self.ctx.app_state)
                        .await?;

                        tracing::info!(
                            context_id = self.ctx.context_id,
                            actionable_id = %fulfilled_id,
                            "Cleared fulfilled actionable from context"
                        );
                    }
                }
            }
        }

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "result": {
                "message": if trigger_execution { "Message relayed and execution triggered" } else { "Message relayed to target context" },
                "target_context_id": target_context_id,
                "target_context_title": target_context.title,
                "relayed_message": message,
                "execution_triggered": trigger_execution
            }
        }))
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

        let execution_id = self.app_state().sf.next_id()? as u64;

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
        let embeddings = embeddings_command.execute(self.app_state()).await?;
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

        let search_results = search_command.execute(self.app_state()).await?;

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

/// Dynamically analyze JSON value and generate a human-readable structure hint.
/// Max depth of 3 levels to keep hints concise.
fn infer_schema_hint(value: &Value) -> String {
    infer_schema_recursive(value, 0)
}

fn infer_schema_recursive(value: &Value, depth: usize) -> String {
    if depth > 5 {
        return "...".to_string();
    }

    match value {
        Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }
            let fields: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", k, infer_type_hint(v, depth + 1)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        Value::Array(arr) => {
            if let Some(first) = arr.first() {
                format!("{}[]", infer_type_hint(first, depth + 1))
            } else {
                "[]".to_string()
            }
        }
        _ => infer_type_hint(value, depth),
    }
}

fn infer_type_hint(value: &Value, depth: usize) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(n) => {
            if n.is_i64() { "int".to_string() }
            else { "number".to_string() }
        }
        Value::String(s) => {
            // Give better hints for common patterns
            if s.contains("T") && s.contains(":") && s.len() > 15 {
                "datetime".to_string()
            } else if s.starts_with("http") {
                "url".to_string()
            } else {
                "string".to_string()
            }
        }
        Value::Array(_) | Value::Object(_) => infer_schema_recursive(value, depth),
    }
}


