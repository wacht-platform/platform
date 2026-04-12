use super::ToolExecutor;
use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use common::error::AppError;
use dto::json::agent_executor::{EditFileParams, ExecuteCommandParams, ReadFileParams, ReadImageParams, WriteFileParams};
use models::AiTool;
use serde_json::Value;

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
    pub(super) async fn execute_read_image(
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

    pub(super) async fn execute_write_file(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: WriteFileParams,
    ) -> Result<Value, AppError> {
        let result = filesystem
            .write_file(&params.path, &params.content, params.append)
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

    pub(super) async fn execute_read_file(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: ReadFileParams,
    ) -> Result<Value, AppError> {
        let result = filesystem
            .read_file(&params.path, params.start_line, params.end_line)
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": params.path,
            "content": result.content,
            "total_lines": result.total_lines,
            "start_line": result.start_line,
            "end_line": result.end_line,
            "slice_hash": result.slice_hash
        }))
    }

    pub(super) async fn execute_edit_file(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: EditFileParams,
    ) -> Result<Value, AppError> {
        let result = filesystem
            .edit_file(
                &params.path,
                &params.new_content,
                params.live_slice_hash.as_deref(),
                params.dangerously_skip_slice_comparison,
                params.start_line,
                params.end_line,
            )
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": params.path,
            "start_line": params.start_line,
            "end_line": params.end_line,
            "live_slice_hash": params.live_slice_hash,
            "dangerously_skip_slice_comparison": params.dangerously_skip_slice_comparison,
            "replaced_content": result.replaced_content,
            "lines_written": result.lines_written,
            "total_lines": result.total_lines,
            "partial": result.partial
        }))
    }

    pub(super) async fn execute_command(
        &self,
        tool: &AiTool,
        shell: &ShellExecutor,
        params: ExecuteCommandParams,
    ) -> Result<Value, AppError> {
        let output = shell.execute(&params.command).await?;

        if output.exit_code != 0 {
            let detail = if !output.stderr.trim().is_empty() {
                output.stderr.trim()
            } else if !output.stdout.trim().is_empty() {
                output.stdout.trim()
            } else {
                "Command produced no output"
            };
            let detail = detail.lines().take(20).collect::<Vec<_>>().join("\n");
            return Err(AppError::Internal(format!(
                "Command exited with code {}: {}",
                output.exit_code, detail
            )));
        }

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "command": params.command,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "exit_code": output.exit_code
        }))
    }
}
