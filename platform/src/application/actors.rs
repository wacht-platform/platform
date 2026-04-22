use commands::CreateActorCommand;
use models::Actor;

use crate::application::{AppError, AppState};

pub struct CreateActorRequest {
    pub subject_type: String,
    pub external_key: String,
    pub display_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn create_actor(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateActorRequest,
) -> Result<Actor, AppError> {
    let id = app_state.sf.next_id()? as i64;

    let mut cmd = CreateActorCommand::new(
        id,
        deployment_id,
        request.subject_type,
        request.external_key,
    )
    .with_metadata(request.metadata.unwrap_or_else(|| serde_json::json!({})));

    if let Some(display_name) = request.display_name {
        cmd = cmd.with_display_name(display_name);
    }

    cmd.execute_with_db(app_state.db_router.writer()).await
}
