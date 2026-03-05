use axum::extract::{Json, Path, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    rate_limit_scheme::{
        CreateRateLimitSchemeCommand, DeleteRateLimitSchemeCommand, UpdateRateLimitSchemeCommand,
    },
};
use common::state::AppState;
use dto::json::api_key::*;
use queries::{
    Query as QueryTrait,
    rate_limit_scheme::{GetRateLimitSchemeQuery, ListRateLimitSchemesQuery, RateLimitSchemeData},
};

pub async fn list_rate_limit_schemes(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ListRateLimitSchemesResponse<RateLimitSchemeData>> {
    let schemes = ListRateLimitSchemesQuery::new(deployment_id)
        .execute(&app_state)
        .await?;

    Ok(ListRateLimitSchemesResponse {
        total: schemes.len(),
        schemes,
    }
    .into())
}

pub async fn get_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme = GetRateLimitSchemeQuery::new(deployment_id, slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Rate limit scheme not found"))?;

    Ok(scheme.into())
}

pub async fn create_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateRateLimitSchemeRequest>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme = CreateRateLimitSchemeCommand {
        deployment_id,
        slug: request.slug,
        name: request.name,
        description: request.description,
        rules: request.rules,
    }
    .execute(&app_state)
    .await?;

    Ok(scheme.into())
}

pub async fn update_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<UpdateRateLimitSchemeRequest>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme = UpdateRateLimitSchemeCommand {
        deployment_id,
        slug,
        name: request.name,
        description: request.description,
        rules: request.rules,
    }
    .execute(&app_state)
    .await?;

    Ok(scheme.into())
}

pub async fn delete_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<()> {
    DeleteRateLimitSchemeCommand {
        deployment_id,
        slug,
    }
    .execute(&app_state)
    .await?;

    Ok((StatusCode::NO_CONTENT, ()).into())
}
