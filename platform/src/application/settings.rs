use commands::{
    CreateDeploymentJwtTemplateCommand, DeleteDeploymentJwtTemplateCommand,
    RemoveDeploymentSmtpConfigCommand, UpdateDeploymentAuthSettingsCommand,
    UpdateDeploymentDisplaySettingsCommand, UpdateDeploymentEmailTemplateCommand,
    UpdateDeploymentJwtTemplateCommand, UpdateDeploymentRestrictionsCommand,
    UpdateDeploymentSmtpConfigCommand, VerifySmtpConnectionCommand,
};
use common::db_router::ReadConsistency;
use common::error::AppError;
use dto::{
    json::{
        DeploymentAuthSettingsUpdates, DeploymentDisplaySettingsUpdates,
        DeploymentRestrictionsUpdates, NewDeploymentJwtTemplate, PartialDeploymentJwtTemplate,
        SmtpConfigRequest, SmtpConfigResponse, SmtpVerifyResponse,
    },
    params::deployment::DeploymentNameParams,
};
use models::{DeploymentJwtTemplate, DeploymentWithSettings, EmailTemplate};
use queries::{
    GetDeploymentEmailTemplateQuery,
    deployment::{GetDeploymentJwtTemplatesQuery, GetDeploymentWithSettingsQuery},
};

use crate::application::AppState;

pub async fn get_deployment_with_settings(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<DeploymentWithSettings, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetDeploymentWithSettingsQuery::new(deployment_id)
        .execute_with(reader)
        .await
}

pub async fn update_deployment_display_settings(
    app_state: &AppState,
    deployment_id: i64,
    updates: DeploymentDisplaySettingsUpdates,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    UpdateDeploymentDisplaySettingsCommand::new(deployment_id, updates)
        .execute_with(writer, &app_state.redis_client)
        .await?;
    Ok(())
}

pub async fn update_deployment_auth_settings(
    app_state: &AppState,
    deployment_id: i64,
    updates: DeploymentAuthSettingsUpdates,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    UpdateDeploymentAuthSettingsCommand::new(deployment_id, updates)
        .execute_with(writer, &app_state.redis_client)
        .await?;
    Ok(())
}

pub async fn update_deployment_restrictions(
    app_state: &AppState,
    deployment_id: i64,
    updates: DeploymentRestrictionsUpdates,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    UpdateDeploymentRestrictionsCommand::new(deployment_id, updates)
        .execute_with(writer, &app_state.redis_client)
        .await?;
    Ok(())
}

pub async fn get_deployment_jwt_templates(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<Vec<DeploymentJwtTemplate>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetDeploymentJwtTemplatesQuery::new(deployment_id)
        .execute_with(reader)
        .await
}

pub async fn create_deployment_jwt_template(
    app_state: &AppState,
    deployment_id: i64,
    template: NewDeploymentJwtTemplate,
) -> Result<DeploymentJwtTemplate, AppError> {
    let writer = app_state.db_router.writer();
    CreateDeploymentJwtTemplateCommand::new(deployment_id, template)
        .execute_with(
            writer,
            app_state.sf.next_id()? as i64,
            &app_state.redis_client,
        )
        .await
}

pub async fn update_deployment_jwt_template(
    app_state: &AppState,
    deployment_id: i64,
    template_id: i64,
    updates: PartialDeploymentJwtTemplate,
) -> Result<DeploymentJwtTemplate, AppError> {
    let writer = app_state.db_router.writer();
    UpdateDeploymentJwtTemplateCommand::new(deployment_id, template_id, updates)
        .execute_with(writer, &app_state.redis_client)
        .await
}

pub async fn delete_deployment_jwt_template(
    app_state: &AppState,
    deployment_id: i64,
    template_id: i64,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    DeleteDeploymentJwtTemplateCommand::new(deployment_id, template_id)
        .execute_with(writer, &app_state.redis_client)
        .await?;
    Ok(())
}

pub async fn get_deployment_email_template(
    app_state: &AppState,
    deployment_id: i64,
    template_name: DeploymentNameParams,
) -> Result<EmailTemplate, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetDeploymentEmailTemplateQuery::new(deployment_id, template_name)
        .execute_with(reader)
        .await
}

pub async fn update_deployment_email_template(
    app_state: &AppState,
    deployment_id: i64,
    template_name: DeploymentNameParams,
    template: EmailTemplate,
) -> Result<EmailTemplate, AppError> {
    let writer = app_state.db_router.writer();
    UpdateDeploymentEmailTemplateCommand::new(deployment_id, template_name, template)
        .execute_with(writer, &app_state.redis_client)
        .await
}

pub async fn verify_smtp_connection(
    _app_state: &AppState,
    config: SmtpConfigRequest,
) -> Result<SmtpVerifyResponse, AppError> {
    VerifySmtpConnectionCommand::new(
        config.host,
        config.port,
        config.username,
        config.password,
        config.from_email,
        config.use_tls,
    )
    .execute_with()
    .await?;

    Ok(SmtpVerifyResponse {
        success: true,
        message: Some("SMTP connection verified successfully".to_string()),
    })
}

pub async fn update_smtp_config(
    app_state: &AppState,
    deployment_id: i64,
    config: SmtpConfigRequest,
) -> Result<SmtpConfigResponse, AppError> {
    VerifySmtpConnectionCommand::new(
        config.host.clone(),
        config.port,
        config.username.clone(),
        config.password.clone(),
        config.from_email.clone(),
        config.use_tls,
    )
    .execute_with()
    .await?;

    let writer = app_state.db_router.writer();
    let result = UpdateDeploymentSmtpConfigCommand::new(
        deployment_id,
        config.host,
        config.port,
        config.username,
        config.password,
        config.from_email,
        config.use_tls,
    )
    .execute_with(writer, &app_state.encryption_service)
    .await?;

    Ok(SmtpConfigResponse {
        host: result.host,
        port: result.port,
        username: result.username,
        from_email: result.from_email,
        use_tls: result.use_tls,
        verified: result.verified,
    })
}

pub async fn remove_smtp_config(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    RemoveDeploymentSmtpConfigCommand::new(deployment_id)
        .execute_with(writer, &app_state.redis_client)
        .await?;
    Ok(())
}
