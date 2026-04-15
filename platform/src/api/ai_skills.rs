use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::api::multipart::MultipartPayload;
use crate::application::{ai_skills as ai_skills_app, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;

pub use ai_skills_app::{SkillFileResponse, SkillTreeResponse};

#[derive(Deserialize)]
pub struct AgentParams {
    pub agent_id: i64,
}

#[derive(Deserialize)]
pub struct AgentSkillParams {
    pub agent_id: i64,
    pub skill_slug: String,
}

#[derive(Deserialize)]
pub struct SkillTreeQuery {
    pub scope: String,
    pub path: Option<String>,
}

pub async fn list_skill_tree(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    Query(query): Query<SkillTreeQuery>,
) -> ApiResult<SkillTreeResponse> {
    let scope = ai_skills_app::SkillScope::parse(&query.scope)?;
    let response = ai_skills_app::list_skill_tree(
        &app_state,
        deployment_id,
        params.agent_id,
        scope,
        query.path.unwrap_or_else(|| "/".to_string()),
    )
    .await?;
    Ok(response.into())
}

pub async fn read_skill_file(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    Query(query): Query<SkillTreeQuery>,
) -> ApiResult<SkillFileResponse> {
    let scope = ai_skills_app::SkillScope::parse(&query.scope)?;
    let path = query
        .path
        .ok_or((StatusCode::BAD_REQUEST, "path is required".to_string()))?;
    let response =
        ai_skills_app::read_skill_file(&app_state, deployment_id, params.agent_id, scope, path)
            .await?;
    Ok(response.into())
}

pub async fn import_agent_skill_bundle(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    multipart: Multipart,
) -> ApiResult<SkillTreeResponse> {
    let payload = MultipartPayload::parse(multipart).await?;

    let mut replace_existing = false;
    let mut file_name: Option<String> = None;
    let mut file_content: Option<Vec<u8>> = None;

    for field in payload.fields() {
        match field.name.as_str() {
            "replace" | "replace_existing" => {
                replace_existing = matches!(
                    field.text()?.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                );
            }
            "file" => {
                file_name = field.file_name.clone();
                file_content = Some(field.bytes.clone());
            }
            _ => {}
        }
    }

    let file_name = file_name.ok_or((StatusCode::BAD_REQUEST, "file is required".to_string()))?;
    let file_content =
        file_content.ok_or((StatusCode::BAD_REQUEST, "file is required".to_string()))?;

    let response = ai_skills_app::import_agent_skill_bundle(
        &app_state,
        deployment_id,
        params.agent_id,
        ai_skills_app::CreateSkillBundleInput {
            file_name,
            file_content,
            replace_existing,
        },
    )
    .await?;

    Ok(response.into())
}

pub async fn delete_agent_skill(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentSkillParams>,
) -> ApiResult<()> {
    ai_skills_app::delete_agent_skill(
        &app_state,
        deployment_id,
        params.agent_id,
        params.skill_slug,
    )
    .await?;
    Ok(().into())
}
