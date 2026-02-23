use commands::Command;
use common::error::AppError;
use common::state::AppState;
use queries::Query;
use serde::Serialize;
use serde_json::Value;

use super::response;
use super::{SpawnControlDirective, SpawnControlRequest};

#[derive(Serialize)]
struct SpawnControlAck {
    message: String,
}

pub async fn spawn_control(
    app_state: &AppState,
    deployment_id: i64,
    sender_context_id: i64,
    tool_name: &str,
    request: SpawnControlRequest,
) -> Result<Value, AppError> {
    let child_context =
        queries::GetExecutionContextQuery::new(request.child_context_id.0, deployment_id)
            .execute(app_state)
            .await?;

    if child_context.parent_context_id != Some(sender_context_id) {
        return Err(AppError::BadRequest(
            "Child context is not owned by the current parent context".to_string(),
        ));
    }

    let action = match request.action {
        SpawnControlDirective::Stop => commands::SpawnControlAction::Stop,
        SpawnControlDirective::Restart => commands::SpawnControlAction::Restart,
        SpawnControlDirective::UpdateParams => commands::SpawnControlAction::UpdateParams(
            request.params.unwrap_or(serde_json::json!({})),
        ),
    };

    commands::PublishSpawnControlCommand::new(request.child_context_id.0, deployment_id, action)
        .with_sender(sender_context_id)
        .execute(app_state)
        .await?;

    response::success(
        tool_name,
        SpawnControlAck {
            message: "Control message sent to child context".to_string(),
        },
    )
}
