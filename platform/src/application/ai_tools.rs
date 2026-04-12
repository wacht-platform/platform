use commands::{
    AttachToolToAgentCommand, CreateAiToolCommand, DeleteAiToolCommand, DetachToolFromAgentCommand,
    UpdateAiToolCommand,
};
use common::ReadConsistency;
use common::error::AppError;
use dto::{
    json::deployment::{CreateToolRequest, UpdateToolRequest},
    query::deployment::GetToolsQuery,
};
use models::{AiTool, AiToolConfiguration, AiToolType, AiToolWithDetails, CodeRunnerEnvVariable};
use queries::{GetAgentToolsQuery, GetAiToolByIdQuery, GetAiToolsQuery};

use crate::{
    api::pagination::paginate_results,
    application::{AppState, response::PaginatedResponse},
};

pub async fn get_ai_tools(
    app_state: &AppState,
    deployment_id: i64,
    query: GetToolsQuery,
) -> Result<PaginatedResponse<AiToolWithDetails>, AppError> {
    let limit = query.limit.unwrap_or(50) as i32;
    let query_limit = limit as u32;
    let offset = query.offset.map(|o| o as i64);

    let tools = GetAiToolsQuery::new(deployment_id)
        .with_limit(Some(query_limit + 1))
        .with_offset(offset.map(|o| o as u32))
        .with_search(query.search)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let tools = tools
        .into_iter()
        .map(|tool| decrypt_tool_with_details(tool, &app_state.encryption_service))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(paginate_results(tools, limit, offset))
}

pub async fn create_ai_tool(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateToolRequest,
) -> Result<AiTool, AppError> {
    let tool_type = AiToolType::from(request.tool_type);
    let tool_id = app_state.sf.next_id()? as i64;
    let configuration =
        encrypt_tool_configuration(request.configuration, &app_state.encryption_service)?;
    CreateAiToolCommand::new(
        tool_id,
        deployment_id,
        request.name,
        request.description,
        tool_type,
        request.requires_user_approval,
        configuration,
    )
    .execute_with_db(app_state.db_router.writer())
    .await
    .and_then(|tool| decrypt_tool(tool, &app_state.encryption_service))
}

pub async fn get_ai_tool_by_id(
    app_state: &AppState,
    deployment_id: i64,
    tool_id: i64,
) -> Result<AiToolWithDetails, AppError> {
    let tool = GetAiToolByIdQuery::new(deployment_id, tool_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    decrypt_tool_with_details(tool, &app_state.encryption_service)
}

pub async fn get_agent_tools(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<PaginatedResponse<AiTool>, AppError> {
    let tools = GetAgentToolsQuery::new(deployment_id, agent_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;
    let tools = tools
        .into_iter()
        .map(|tool| decrypt_tool(tool, &app_state.encryption_service))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PaginatedResponse::from(tools))
}

pub async fn attach_tool_to_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    tool_id: i64,
) -> Result<(), AppError> {
    AttachToolToAgentCommand::new(deployment_id, agent_id, tool_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn detach_tool_from_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    tool_id: i64,
) -> Result<(), AppError> {
    DetachToolFromAgentCommand::new(deployment_id, agent_id, tool_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn update_ai_tool(
    app_state: &AppState,
    deployment_id: i64,
    tool_id: i64,
    request: UpdateToolRequest,
) -> Result<AiTool, AppError> {
    let mut command = UpdateAiToolCommand::new(deployment_id, tool_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }
    if let Some(description) = request.description {
        command = command.with_description(Some(description));
    }
    if let Some(tool_type) = request.tool_type {
        command = command.with_tool_type(AiToolType::from(tool_type));
    }
    if let Some(requires_user_approval) = request.requires_user_approval {
        command = command.with_requires_user_approval(requires_user_approval);
    }
    if let Some(configuration) = request.configuration {
        command = command.with_configuration(encrypt_tool_configuration(
            configuration,
            &app_state.encryption_service,
        )?);
    }

    command
        .execute_with_db(app_state.db_router.writer())
        .await
        .and_then(|tool| decrypt_tool(tool, &app_state.encryption_service))
}

pub async fn delete_ai_tool(
    app_state: &AppState,
    deployment_id: i64,
    tool_id: i64,
) -> Result<(), AppError> {
    DeleteAiToolCommand::new(deployment_id, tool_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}

fn encrypt_tool_with_envs(
    envs: Option<Vec<CodeRunnerEnvVariable>>,
    encryption: &common::EncryptionService,
) -> Result<Option<Vec<CodeRunnerEnvVariable>>, AppError> {
    envs.map(|variables| {
        variables
            .into_iter()
            .map(|variable| {
                Ok(CodeRunnerEnvVariable {
                    name: variable.name,
                    value: encryption.encrypt(&variable.value)?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()
    })
    .transpose()
}

fn decrypt_tool_with_envs(
    envs: Option<Vec<CodeRunnerEnvVariable>>,
    encryption: &common::EncryptionService,
) -> Result<Option<Vec<CodeRunnerEnvVariable>>, AppError> {
    envs.map(|variables| {
        variables
            .into_iter()
            .map(|variable| {
                Ok(CodeRunnerEnvVariable {
                    name: variable.name,
                    value: encryption.decrypt(&variable.value)?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()
    })
    .transpose()
}

fn encrypt_tool_configuration(
    configuration: AiToolConfiguration,
    encryption: &common::EncryptionService,
) -> Result<AiToolConfiguration, AppError> {
    match configuration {
        AiToolConfiguration::CodeRunner(mut config) => {
            config.env_variables = encrypt_tool_with_envs(config.env_variables, encryption)?;
            Ok(AiToolConfiguration::CodeRunner(config))
        }
        other => Ok(other),
    }
}

fn decrypt_tool_configuration(
    configuration: AiToolConfiguration,
    encryption: &common::EncryptionService,
) -> Result<AiToolConfiguration, AppError> {
    match configuration {
        AiToolConfiguration::CodeRunner(mut config) => {
            config.env_variables = decrypt_tool_with_envs(config.env_variables, encryption)?;
            Ok(AiToolConfiguration::CodeRunner(config))
        }
        other => Ok(other),
    }
}

fn decrypt_tool(
    mut tool: AiTool,
    encryption: &common::EncryptionService,
) -> Result<AiTool, AppError> {
    tool.configuration = decrypt_tool_configuration(tool.configuration, encryption)?;
    Ok(tool)
}

fn decrypt_tool_with_details(
    mut tool: AiToolWithDetails,
    encryption: &common::EncryptionService,
) -> Result<AiToolWithDetails, AppError> {
    tool.configuration = decrypt_tool_configuration(tool.configuration, encryption)?;
    Ok(tool)
}
