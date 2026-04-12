use super::ToolExecutor;
use crate::filesystem::{
    sandbox::{SandboxExecutionOptions, SandboxRunner},
    AgentFilesystem,
};
use common::error::AppError;
use models::{AiTool, CodeRunnerRuntime, CodeRunnerToolConfiguration, SchemaField};
use serde::Serialize;
use serde_json::{Map, Value};
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Serialize)]
pub(super) struct CodeRunnerToolResult {
    success: bool,
    tool: String,
    runtime: String,
    output: Value,
}

impl ToolExecutor {
    pub(super) async fn execute_code_runner_tool(
        &self,
        tool: &AiTool,
        config: &CodeRunnerToolConfiguration,
        execution_params: &Value,
        filesystem: &AgentFilesystem,
    ) -> Result<CodeRunnerToolResult, AppError> {
        let input = collect_code_runner_input(config, execution_params)?;
        let timeout_secs = config.timeout_seconds.unwrap_or(30) as u64;

        let runner_root = filesystem.execution_root().join("code_runner");
        fs::create_dir_all(&runner_root).await.map_err(|e| {
            AppError::Internal(format!("Failed to prepare code runner root: {}", e))
        })?;

        let run_id = self.app_state().sf.next_id()?;
        let script_path = runner_root.join(format!("code_runner_{}.py", run_id));
        let input_path = runner_root.join(format!("code_runner_input_{}.json", run_id));
        let output_path = runner_root.join(format!("code_runner_output_{}.json", run_id));
        let wrapper_path = runner_root.join(format!("code_runner_wrapper_{}.py", run_id));

        fs::write(&script_path, &config.code).await.map_err(|e| {
            AppError::Internal(format!("Failed to write code runner script: {}", e))
        })?;
        fs::write(&input_path, serde_json::to_vec_pretty(&input)?)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write code runner input: {}", e)))?;
        fs::write(&wrapper_path, PYTHON_CODE_RUNNER_WRAPPER)
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to write code runner wrapper: {}", e))
            })?;

        let sandbox = SandboxRunner::new(filesystem.execution_root());
        let options = SandboxExecutionOptions {
            allow_network: config.allow_network,
            writable_workspace: true,
            writable_scratch: false,
            extra_read_only_paths: code_runner_runtime_read_paths(),
            extra_env: self.code_runner_execution_env(config)?,
        };
        let wrapper_arg = code_runner_exec_arg(&sandbox, &wrapper_path);
        let script_arg = code_runner_exec_arg(&sandbox, &script_path);
        let input_arg = code_runner_exec_arg(&sandbox, &input_path);
        let output_arg = code_runner_exec_arg(&sandbox, &output_path);
        let python_path = code_runner_python_path();
        let output = sandbox
            .execute_program_with_options(
                &python_path,
                &[wrapper_arg, script_arg, input_arg, output_arg],
                timeout_secs,
                options,
            )
            .await?;

        if output.exit_code != 0 {
            return Err(AppError::Internal(format_code_runner_failure(
                &tool.name,
                output.exit_code,
                &output.stderr,
                &output.stdout,
            )));
        }

        let output_bytes = fs::read(&output_path).await.map_err(|e| {
            AppError::Internal(format!(
                "Code runner '{}' did not produce an output file: {}",
                tool.name, e
            ))
        })?;
        let parsed_output = parse_code_runner_output(&output_bytes, &tool.name)?;
        validate_code_runner_output(config, &parsed_output)?;

        Ok(CodeRunnerToolResult {
            success: true,
            tool: tool.name.clone(),
            runtime: match config.runtime {
                CodeRunnerRuntime::Python => "python".to_string(),
            },
            output: parsed_output,
        })
    }
}

impl ToolExecutor {
    fn code_runner_execution_env(
        &self,
        config: &CodeRunnerToolConfiguration,
    ) -> Result<Vec<(String, String)>, AppError> {
        let mut env = self.code_runner_provider_env()?;
        env.extend(self.code_runner_tool_env(config)?);
        Ok(env)
    }

    fn code_runner_provider_env(&self) -> Result<Vec<(String, String)>, AppError> {
        let mut env = Vec::new();

        if let Some(key) = self.ctx.provider_keys.openai_api_key.clone() {
            env.push(("OPENAI_API_KEY".to_string(), key));
        }
        if let Some(key) = self.ctx.provider_keys.anthropic_api_key.clone() {
            env.push(("ANTHROPIC_API_KEY".to_string(), key));
        }
        if let Some(key) = self.ctx.provider_keys.gemini_api_key.clone() {
            env.push(("GEMINI_API_KEY".to_string(), key));
        }

        Ok(env)
    }

    fn code_runner_tool_env(
        &self,
        config: &CodeRunnerToolConfiguration,
    ) -> Result<Vec<(String, String)>, AppError> {
        let encryption = &self.app_state().encryption_service;
        let mut env = Vec::new();

        if let Some(variables) = &config.env_variables {
            for variable in variables {
                env.push((variable.name.clone(), encryption.decrypt(&variable.value)?));
            }
        }

        Ok(env)
    }
}

fn collect_code_runner_input(
    config: &CodeRunnerToolConfiguration,
    execution_params: &Value,
) -> Result<Value, AppError> {
    let mut input = Map::new();

    if let Some(schema) = &config.input_schema {
        for field in schema {
            match execution_params.get(&field.name) {
                Some(value) => {
                    validate_schema_field_value(value, field, "input")?;
                    input.insert(field.name.clone(), value.clone());
                }
                None if field.required => {
                    return Err(AppError::BadRequest(format!(
                        "Missing required code runner input field '{}'",
                        field.name
                    )));
                }
                None => {}
            }
        }
    } else if let Some(obj) = execution_params.as_object() {
        input.extend(obj.clone());
    } else {
        return Err(AppError::BadRequest(
            "Code runner execution parameters must be an object".to_string(),
        ));
    }

    Ok(Value::Object(input))
}

fn code_runner_exec_arg(sandbox: &SandboxRunner, path: &std::path::Path) -> String {
    if sandbox.uses_virtual_alias_paths() {
        format!(
            "/app/code_runner/{}",
            path.file_name().unwrap().to_string_lossy()
        )
    } else {
        path.to_string_lossy().to_string()
    }
}

fn code_runner_python_path() -> String {
    if let Ok(path) = env::var("CODE_RUNNER_PYTHON_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if cfg!(target_os = "macos") {
        let platform_api_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("agent-engine crate should live under platform-api");
        return platform_api_root
            .join(".runtime/code_runner/venv/bin/python")
            .to_string_lossy()
            .to_string();
    }

    "/opt/wacht/code_runner/venv/bin/python".to_string()
}

fn code_runner_runtime_read_paths() -> Vec<PathBuf> {
    let python_path = PathBuf::from(code_runner_python_path());
    let mut paths = Vec::new();

    if let Some(env_root) = code_runner_env_root(&python_path) {
        paths.push(env_root);
    }

    paths
}

fn code_runner_env_root(python_path: &Path) -> Option<PathBuf> {
    let bin_dir = python_path.parent()?;
    let env_root = bin_dir.parent()?;
    Some(env_root.to_path_buf())
}

fn parse_code_runner_output(output: &[u8], tool_name: &str) -> Result<Value, AppError> {
    serde_json::from_slice(output).map_err(|e| {
        AppError::Internal(format!(
            "Code runner '{}' returned invalid JSON output: {}",
            tool_name, e
        ))
    })
}

fn format_code_runner_failure(
    tool_name: &str,
    exit_code: i32,
    stderr: &str,
    stdout: &str,
) -> String {
    let stderr = stderr.trim();
    let stdout = stdout.trim();

    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "no error output captured"
    };

    format!(
        "Code runner '{}' failed with exit code {}: {}",
        tool_name, exit_code, detail
    )
}

fn validate_code_runner_output(
    config: &CodeRunnerToolConfiguration,
    output: &Value,
) -> Result<(), AppError> {
    let Some(schema) = &config.output_schema else {
        return Ok(());
    };

    let Some(output_object) = output.as_object() else {
        return Err(AppError::Internal(
            "Code runner output must be a JSON object when output_schema is defined".to_string(),
        ));
    };

    let allowed_fields = schema
        .iter()
        .map(|field| field.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    let extra_fields = output_object
        .keys()
        .filter(|key| !allowed_fields.contains(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !extra_fields.is_empty() {
        return Err(AppError::Internal(format!(
            "Code runner output contains undeclared fields: {}",
            extra_fields.join(", ")
        )));
    }

    for field in schema {
        match output_object.get(&field.name) {
            Some(value) => validate_schema_field_value(value, field, "output")?,
            None if field.required => {
                return Err(AppError::Internal(format!(
                    "Code runner output missing required field '{}'",
                    field.name
                )));
            }
            None => {}
        }
    }

    Ok(())
}

fn validate_schema_field_value(
    value: &Value,
    field: &SchemaField,
    label: &str,
) -> Result<(), AppError> {
    let valid = match field.field_type.as_str() {
        "STRING" => value.is_string(),
        "INTEGER" => value.as_i64().is_some() || value.as_u64().is_some(),
        "NUMBER" => value.is_number(),
        "BOOLEAN" => value.is_boolean(),
        "OBJECT" => value.is_object(),
        "ARRAY" => {
            if let Some(items) = value.as_array() {
                if let Some(items_schema) = &field.items_schema {
                    items
                        .iter()
                        .all(|item| validate_schema_field_value(item, items_schema, label).is_ok())
                } else if let Some(items_type) = &field.items_type {
                    items.iter().all(|item| {
                        validate_schema_field_value(
                            item,
                            &SchemaField {
                                name: field.name.clone(),
                                field_type: items_type.clone(),
                                required: true,
                                description: field.description.clone(),
                                ..Default::default()
                            },
                            label,
                        )
                        .is_ok()
                    })
                } else {
                    true
                }
            } else {
                false
            }
        }
        _ => true,
    };

    if valid && field.field_type == "OBJECT" {
        if let Some(properties) = &field.properties {
            let Some(object) = value.as_object() else {
                return Err(AppError::BadRequest(format!(
                    "Invalid {} field '{}' for type {}",
                    label, field.name, field.field_type
                )));
            };

            let allowed_properties = properties
                .iter()
                .map(|property| property.name.as_str())
                .collect::<std::collections::HashSet<_>>();
            let extra_properties = object
                .keys()
                .filter(|key| !allowed_properties.contains(key.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            if !extra_properties.is_empty() {
                return Err(AppError::BadRequest(format!(
                    "Code runner {} object '{}' contains undeclared fields: {}",
                    label,
                    field.name,
                    extra_properties.join(", ")
                )));
            }

            for property in properties {
                match object.get(&property.name) {
                    Some(nested) => validate_schema_field_value(nested, property, label)?,
                    None if property.required => {
                        return Err(AppError::BadRequest(format!(
                            "Missing required {} field '{}.{}'",
                            label, field.name, property.name
                        )))
                    }
                    None => {}
                }
            }
        }
    }

    if valid {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "Invalid {} field '{}' for type {}",
            label, field.name, field.field_type
        )))
    }
}

const PYTHON_CODE_RUNNER_WRAPPER: &str = r#"import asyncio
import inspect
import json
import os
import sys
import traceback
from pathlib import Path

from types import SimpleNamespace


class _UnavailableClient:
    def __init__(self, provider: str):
        self._provider = provider

    def __getattr__(self, name):
        raise RuntimeError(f"{self._provider} is not configured for this deployment")


def _build_openai_client(api_key: str):
    from openai import OpenAI
    return OpenAI(api_key=api_key)


def _build_anthropic_client(api_key: str):
    import anthropic
    return anthropic.Anthropic(api_key=api_key)


def _build_gemini_client(api_key: str):
    from google import genai
    return genai.Client(api_key=api_key)


def _build_clients():
    namespace = SimpleNamespace()

    openai_key = os.environ.get("OPENAI_API_KEY", "").strip()
    if openai_key:
        namespace.openai = _build_openai_client(openai_key)
    else:
        namespace.openai = _UnavailableClient("OpenAI")

    anthropic_key = os.environ.get("ANTHROPIC_API_KEY", "").strip()
    if anthropic_key:
        namespace.anthropic = _build_anthropic_client(anthropic_key)
    else:
        namespace.anthropic = _UnavailableClient("Anthropic")

    gemini_key = os.environ.get("GEMINI_API_KEY", "").strip()
    if gemini_key:
        namespace.gemini = _build_gemini_client(gemini_key)
    else:
        namespace.gemini = _UnavailableClient("Gemini")

    return namespace


def main():
    if len(sys.argv) != 4:
        raise RuntimeError("expected script path, input path, and output path")

    script_path = Path(sys.argv[1])
    input_path = Path(sys.argv[2])
    output_path = Path(sys.argv[3])
    client = _build_clients()
    namespace = {
        "client": client,
        "openai_client": client.openai,
        "anthropic_client": client.anthropic,
        "gemini_client": client.gemini,
    }
    code = script_path.read_text()
    exec(compile(code, str(script_path), "exec"), namespace)

    run_fn = namespace.get("run")
    if not callable(run_fn):
        raise RuntimeError("CodeRunner script must define a callable run(input) function")

    payload = json.loads(input_path.read_text())
    result = run_fn(payload)
    if inspect.isawaitable(result):
        result = asyncio.run(result)
    output_path.write_text(json.dumps(result))


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        sys.stderr.write(json.dumps({
            "error": str(exc),
            "traceback": traceback.format_exc(),
        }))
        sys.exit(1)
"#;
