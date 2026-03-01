use super::{parse_params, ToolExecutor};
use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use common::error::AppError;
use models::{AiTool, InternalToolType};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct ReadFileParams {
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct WriteFileParams {
    path: String,
    #[serde(default)]
    content: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ListDirectoryParams {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchFilesParams {
    query: String,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExecuteCommandParams {
    command: String,
}

#[derive(Debug, Deserialize)]
struct ReadImageParams {
    path: String,
}

impl ToolExecutor {
    pub(super) async fn execute_filesystem_tool(
        &self,
        tool: &AiTool,
        tool_type: InternalToolType,
        execution_params: &Value,
        filesystem: &AgentFilesystem,
        shell: &ShellExecutor,
    ) -> Result<Value, AppError> {
        match tool_type {
            InternalToolType::ReadFile => {
                let params: ReadFileParams = parse_params(execution_params, "read_file")?;
                self.execute_read_file(tool, filesystem, shell, params)
                    .await
            }
            InternalToolType::ReadImage => {
                let params: ReadImageParams = parse_params(execution_params, "read_image")?;
                self.execute_read_image(tool, filesystem, params).await
            }
            InternalToolType::WriteFile => {
                let params: WriteFileParams = parse_params(execution_params, "write_file")?;
                self.execute_write_file(tool, filesystem, params).await
            }
            InternalToolType::ListDirectory => {
                let params: ListDirectoryParams = parse_params(execution_params, "list_directory")?;
                self.execute_list_directory(tool, filesystem, params).await
            }
            InternalToolType::SearchFiles => {
                let params: SearchFilesParams = parse_params(execution_params, "search_files")?;
                self.execute_search_files(tool, filesystem, params).await
            }
            InternalToolType::ExecuteCommand => {
                let params: ExecuteCommandParams =
                    parse_params(execution_params, "execute_command")?;
                self.execute_command(tool, shell, params).await
            }
            _ => Err(AppError::Internal(
                "Unsupported filesystem tool type".to_string(),
            )),
        }
    }

    async fn execute_read_file(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        shell: &ShellExecutor,
        params: ReadFileParams,
    ) -> Result<Value, AppError> {
        let ReadFileParams {
            path,
            start_line,
            end_line,
        } = params;

        let extension = path.rsplit('.').next().unwrap_or_default().to_lowercase();

        match extension.as_str() {
            "txt" | "md" | "json" | "yaml" | "yml" | "csv" | "xml" | "html" | "htm" | "js"
            | "ts" | "jsx" | "tsx" | "py" | "rs" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
            | "css" | "scss" | "toml" | "ini" | "cfg" | "conf" | "sh" | "bash" | "zsh" | "sql"
            | "graphql" | "proto" | "env" | "gitignore" | "dockerfile" | "makefile" | "log"
            | "" => {
                let result = filesystem.read_file(&path, start_line, end_line).await?;
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
                let full_path = filesystem.resolve_path_public(&path)?;
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
                    "base64_included": false,
                    "hint": "Use read_image with the same path to fetch one-time base64 payload for vision analysis."
                }))
            }
            _ => {
                let bytes = filesystem.read_file_bytes(&path).await?;
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

    async fn execute_read_image(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: ReadImageParams,
    ) -> Result<Value, AppError> {
        let path = params.path;
        let extension = path.rsplit('.').next().unwrap_or_default().to_lowercase();
        let mime_type = match extension.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            _ => {
                return Err(AppError::BadRequest(
                    "read_image only supports png, jpg, jpeg, webp, gif, bmp, svg files"
                        .to_string(),
                ));
            }
        };

        use base64::{Engine, engine::general_purpose::STANDARD};
        let bytes = filesystem.read_file_bytes(&path).await?;
        let base64_data = STANDARD.encode(&bytes);

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": path,
            "file_type": "image",
            "mime_type": mime_type,
            "size_bytes": bytes.len(),
            "one_time": true,
            "base64": base64_data,
            "note": "One-time base64 payload for image analysis."
        }))
    }

    async fn execute_write_file(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: WriteFileParams,
    ) -> Result<Value, AppError> {
        let result = filesystem
            .write_file(
                &params.path,
                &params.content,
                params.start_line,
                params.end_line,
            )
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": params.path,
            "lines_written": result.lines_written,
            "total_lines": result.total_lines,
            "partial": result.partial
        }))
    }

    async fn execute_list_directory(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: ListDirectoryParams,
    ) -> Result<Value, AppError> {
        let path = params.path.unwrap_or_else(|| "/".to_string());
        let files = filesystem.list_dir(&path).await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": path,
            "files": files
        }))
    }

    async fn execute_search_files(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: SearchFilesParams,
    ) -> Result<Value, AppError> {
        let path = params.path.unwrap_or_else(|| "/".to_string());
        let result = filesystem.search(&params.query, &path).await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": path,
            "query": params.query,
            "matches": result
        }))
    }

    async fn execute_command(
        &self,
        tool: &AiTool,
        shell: &ShellExecutor,
        params: ExecuteCommandParams,
    ) -> Result<Value, AppError> {
        let output = shell.execute(&params.command).await?;

        Ok(serde_json::json!({
            "success": output.exit_code == 0,
            "tool": tool.name,
            "command": params.command,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "exit_code": output.exit_code
        }))
    }
}
