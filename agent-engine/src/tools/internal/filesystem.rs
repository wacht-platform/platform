use super::ToolExecutor;
use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use common::error::AppError;
use dto::json::agent_executor::{
    AppendFileParams, EditFileParams, ExecuteCommandParams, ReadFileParams, ReadImageParams,
    WriteFileParams,
};
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
            .write_file(&params.path, &params.content, false)
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

    pub(super) async fn execute_append_file(
        &self,
        tool: &AiTool,
        filesystem: &AgentFilesystem,
        params: AppendFileParams,
    ) -> Result<Value, AppError> {
        let result = filesystem
            .write_file(&params.path, &params.content, true)
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
            .read_file(
                &params.path,
                params.start_line,
                params.end_line,
                params.start_char,
                params.end_char,
            )
            .await?;

        let mut out = serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": params.path,
            "content": result.content,
            "total_lines": result.total_lines,
            "total_chars": result.total_chars,
            "slice_hash": result.slice_hash,
        });
        let obj = out.as_object_mut().expect("json object");
        match (result.start_char, result.end_char) {
            (Some(start_char), Some(end_char)) => {
                obj.insert("start_char".into(), serde_json::json!(start_char));
                obj.insert("end_char".into(), serde_json::json!(end_char));
            }
            _ => {
                obj.insert("start_line".into(), serde_json::json!(result.start_line));
                obj.insert("end_line".into(), serde_json::json!(result.end_line));
            }
        }
        Ok(out)
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
                &params.old_string,
                &params.new_string,
                params.replace_all,
            )
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "path": params.path,
            "replacements": result.replacements,
            "total_lines": result.total_lines,
        }))
    }

    pub(super) async fn execute_command(
        &self,
        tool: &AiTool,
        shell: &ShellExecutor,
        params: ExecuteCommandParams,
    ) -> Result<Value, AppError> {
        let output = shell
            .execute_with_timeout(&params.command, params.timeout_seconds)
            .await?;

        Ok(serde_json::json!({
            "success": output.exit_code == 0,
            "tool": tool.name,
            "command": params.command,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "exit_code": output.exit_code,
            "timeout_seconds": params.timeout_seconds,
        }))
    }
}
