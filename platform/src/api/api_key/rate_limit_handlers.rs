use axum::extract::{Json, Path, State};
use axum::http::StatusCode;

use crate::application::{api_key_rate_limit as api_key_rate_limit_app, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::*;
use queries::rate_limit_scheme::RateLimitSchemeData;

pub async fn list_rate_limit_schemes(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ListRateLimitSchemesResponse<RateLimitSchemeData>> {
    let schemes =
        api_key_rate_limit_app::list_rate_limit_schemes(&app_state, deployment_id).await?;
    Ok(schemes.into())
}

pub async fn get_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme =
        api_key_rate_limit_app::get_rate_limit_scheme(&app_state, deployment_id, slug)
            .await?;
    Ok(scheme.into())
}

pub async fn create_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateRateLimitSchemeRequest>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme =
        api_key_rate_limit_app::create_rate_limit_scheme(&app_state, deployment_id, request)
            .await?;
    Ok(scheme.into())
}

pub async fn update_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<UpdateRateLimitSchemeRequest>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme = api_key_rate_limit_app::update_rate_limit_scheme(
        &app_state,
        deployment_id,
        slug,
        request,
    )
    .await?;

    Ok(scheme.into())
}

pub async fn delete_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<()> {
    api_key_rate_limit_app::delete_rate_limit_scheme(&app_state, deployment_id, slug).await?;
    Ok((StatusCode::NO_CONTENT, ()).into())
}
