use commands::{
    Command, CreateDeploymentJwtTemplateCommand, DeleteDeploymentJwtTemplateCommand,
    RemoveDeploymentSmtpConfigCommand, UpdateDeploymentAuthSettingsCommand,
    UpdateDeploymentDisplaySettingsCommand, UpdateDeploymentEmailTemplateCommand,
    UpdateDeploymentJwtTemplateCommand, UpdateDeploymentRestrictionsCommand,
    UpdateDeploymentSmtpConfigCommand, VerifySmtpConnectionCommand,
};
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
    GetDeploymentEmailTemplateQuery, Query,
    deployment::{GetDeploymentJwtTemplatesQuery, GetDeploymentWithSettingsQuery},
};

use crate::application::AppState;

pub async fn get_deployment_with_settings(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<DeploymentWithSettings, AppError> {
    GetDeploymentWithSettingsQuery::new(deployment_id)
        .execute(app_state)
        .await
}

pub async fn update_deployment_display_settings(
    app_state: &AppState,
    deployment_id: i64,
    updates: DeploymentDisplaySettingsUpdates,
) -> Result<(), AppError> {
    UpdateDeploymentDisplaySettingsCommand::new(deployment_id, updates)
        .execute(app_state)
        .await?;
    Ok(())
}

pub async fn update_deployment_auth_settings(
    app_state: &AppState,
    deployment_id: i64,
    updates: DeploymentAuthSettingsUpdates,
) -> Result<(), AppError> {
    UpdateDeploymentAuthSettingsCommand::new(deployment_id, updates)
        .execute(app_state)
        .await?;
    Ok(())
}

pub async fn update_deployment_restrictions(
    app_state: &AppState,
    deployment_id: i64,
    updates: DeploymentRestrictionsUpdates,
) -> Result<(), AppError> {
    UpdateDeploymentRestrictionsCommand::new(deployment_id, updates)
        .execute(app_state)
        .await?;
    Ok(())
}

pub async fn get_deployment_jwt_templates(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<Vec<DeploymentJwtTemplate>, AppError> {
    GetDeploymentJwtTemplatesQuery::new(deployment_id)
        .execute(app_state)
        .await
}

pub async fn create_deployment_jwt_template(
    app_state: &AppState,
    deployment_id: i64,
    template: NewDeploymentJwtTemplate,
) -> Result<DeploymentJwtTemplate, AppError> {
    CreateDeploymentJwtTemplateCommand::new(deployment_id, template)
        .execute(app_state)
        .await
}

pub async fn update_deployment_jwt_template(
    app_state: &AppState,
    deployment_id: i64,
    template_id: i64,
    updates: PartialDeploymentJwtTemplate,
) -> Result<DeploymentJwtTemplate, AppError> {
    UpdateDeploymentJwtTemplateCommand::new(deployment_id, template_id, updates)
        .execute(app_state)
        .await
}

pub async fn delete_deployment_jwt_template(
    app_state: &AppState,
    deployment_id: i64,
    template_id: i64,
) -> Result<(), AppError> {
    DeleteDeploymentJwtTemplateCommand::new(deployment_id, template_id)
        .execute(app_state)
        .await?;
    Ok(())
}

pub async fn get_deployment_email_template(
    app_state: &AppState,
    deployment_id: i64,
    template_name: DeploymentNameParams,
) -> Result<EmailTemplate, AppError> {
    GetDeploymentEmailTemplateQuery::new(deployment_id, template_name)
        .execute(app_state)
        .await
}

pub async fn update_deployment_email_template(
    app_state: &AppState,
    deployment_id: i64,
    template_name: DeploymentNameParams,
    template: EmailTemplate,
) -> Result<EmailTemplate, AppError> {
    UpdateDeploymentEmailTemplateCommand::new(deployment_id, template_name, template)
        .execute(app_state)
        .await
}

pub async fn verify_smtp_connection(
    app_state: &AppState,
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
    .execute(app_state)
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
    .execute(app_state)
    .await?;

    let result = UpdateDeploymentSmtpConfigCommand::new(
        deployment_id,
        config.host,
        config.port,
        config.username,
        config.password,
        config.from_email,
        config.use_tls,
    )
    .execute(app_state)
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
    RemoveDeploymentSmtpConfigCommand::new(deployment_id)
        .execute(app_state)
        .await?;
    Ok(())
}
