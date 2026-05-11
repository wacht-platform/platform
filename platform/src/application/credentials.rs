use std::env;

use commands::api_key::CreateApiKeyCommand;
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::credentials::{DeploymentCredentialsApiKey, DeploymentCredentialsResponse};
use models::error::AppError;
use queries::api_key::GetApiAuthAppBySlugQuery;
use queries::deployment::GetDeploymentWithSettingsQuery;

const BENCH_KEY_NAME: &str = "Bench credentials";

fn console_deployment_id() -> Result<i64, AppError> {
    let raw = env::var("CONSOLE_DEPLOYMENT_ID").map_err(|_| {
        AppError::Internal("CONSOLE_DEPLOYMENT_ID environment variable is not set".to_string())
    })?;
    raw.parse::<i64>().map_err(|e| {
        AppError::Internal(format!(
            "CONSOLE_DEPLOYMENT_ID must be a valid i64, got '{}': {}",
            raw, e
        ))
    })
}

pub async fn create_deployment_credentials(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<DeploymentCredentialsResponse, AppError> {
    let console_id = console_deployment_id()?;
    let system_app_slug = format!("aa_{}", deployment_id);

    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let deployment = GetDeploymentWithSettingsQuery::new(deployment_id)
        .execute_with_db(reader)
        .await?;

    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let app = GetApiAuthAppBySlugQuery::new(console_id, system_app_slug.clone())
        .execute_with_db(reader)
        .await?
        .ok_or_else(|| {
            AppError::Internal(format!(
                "System backend app '{}' not found for deployment {}. Was the deployment fully provisioned?",
                system_app_slug, deployment_id
            ))
        })?;

    let writer = app_state.db_router.writer();
    let command = CreateApiKeyCommand::new(
        app.app_slug.clone(),
        console_id,
        BENCH_KEY_NAME.to_string(),
        app.key_prefix.clone(),
    );

    let created = command
        .with_key_id(app_state.sf.next_id()? as i64)
        .execute_with_db(writer)
        .await?;

    Ok(DeploymentCredentialsResponse {
        publishable_key: deployment.publishable_key,
        frontend_host: deployment.frontend_host,
        backend_host: deployment.backend_host,
        api_key: DeploymentCredentialsApiKey {
            id: created.key.id.to_string(),
            secret: created.secret,
            prefix: created.key.key_prefix,
            suffix: created.key.key_suffix,
            app_slug: app.app_slug,
        },
    })
}
