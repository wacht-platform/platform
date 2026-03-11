use super::{parse_params, ToolExecutor};
use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use common::error::AppError;
use models::{AiTool, InternalToolType};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct WriteFileParams {
    path: String,
    #[serde(default)]
    content: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ExecuteCommandParams {
    command: String,
}

#[derive(Debug, Deserialize)]
struct ReadImageParams {
    path: String,
}

fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.len() >= 3 && bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 2 && bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if let Ok(prefix) = std::str::from_utf8(&bytes[..bytes.len().min(256)]) {
        let t = prefix.trim_start();
        if t.starts_with("<svg") || t.starts_with("<?xml") {
            return Some("image/svg+xml");
        }
    }
    None
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
            InternalToolType::ReadImage => {
                let params: ReadImageParams = parse_params(execution_params, "read_image")?;
                self.execute_read_image(tool, filesystem, params).await
            }
            InternalToolType::WriteFile => {
                let params: WriteFileParams = parse_params(execution_params, "write_file")?;
                self.execute_write_file(tool, filesystem, params).await
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

    async fn execute_read_image(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: ReadImageParams,
    ) -> Result<Value, AppError> {
        let path = params.path;
        let bytes = filesystem.read_file_bytes(&path).await?;
        let mime_type = match sniff_image_mime(&bytes) {
            Some(mime) => mime,
            None => {
                return Err(AppError::BadRequest(
                    "read_image supports only valid png, jpg, jpeg, webp, gif, bmp, svg files"
                        .to_string(),
                ));
            }
        };
        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": path,
            "file_type": "image",
            "mime_type": mime_type,
            "size_bytes": bytes.len()
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
