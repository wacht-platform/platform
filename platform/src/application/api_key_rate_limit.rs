use commands::rate_limit_scheme::{
    CreateRateLimitSchemeCommand, DeleteRateLimitSchemeCommand, UpdateRateLimitSchemeCommand,
};
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::api_key::{
    CreateRateLimitSchemeRequest, ListRateLimitSchemesResponse, UpdateRateLimitSchemeRequest,
};
use models::error::AppError;
use queries::rate_limit_scheme::{
    GetRateLimitSchemeQuery, ListRateLimitSchemesQuery, RateLimitSchemeData,
};

pub async fn list_rate_limit_schemes(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<ListRateLimitSchemesResponse<RateLimitSchemeData>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let schemes = ListRateLimitSchemesQuery::new(deployment_id)
        .execute_with(reader)
        .await?;

    Ok(ListRateLimitSchemesResponse {
        total: schemes.len(),
        schemes,
    })
}

pub async fn get_rate_limit_scheme(
    app_state: &AppState,
    deployment_id: i64,
    slug: String,
) -> Result<RateLimitSchemeData, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetRateLimitSchemeQuery::new(deployment_id, slug)
        .execute_with(reader)
        .await?
        .ok_or_else(|| AppError::NotFound("Rate limit scheme not found".to_string()))
}

pub async fn create_rate_limit_scheme(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateRateLimitSchemeRequest,
) -> Result<RateLimitSchemeData, AppError> {
    let writer = app_state.db_router.writer();
    let scheme = CreateRateLimitSchemeCommand {
        deployment_id,
        slug: request.slug,
        name: request.name,
        description: request.description,
        rules: request.rules,
    }
    .execute_with(writer, app_state.sf.next_id()? as i64)
    .await?;

    Ok(scheme)
}

pub async fn update_rate_limit_scheme(
    app_state: &AppState,
    deployment_id: i64,
    slug: String,
    request: UpdateRateLimitSchemeRequest,
) -> Result<RateLimitSchemeData, AppError> {
    let writer = app_state.db_router.writer();
    let scheme = UpdateRateLimitSchemeCommand {
        deployment_id,
        slug,
        name: request.name,
        description: request.description,
        rules: request.rules,
    }
    .execute_with(writer)
    .await?;

    Ok(scheme)
}

pub async fn delete_rate_limit_scheme(
    app_state: &AppState,
    deployment_id: i64,
    slug: String,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    DeleteRateLimitSchemeCommand {
        deployment_id,
        slug,
    }
    .execute_with(writer)
    .await?;

    Ok(())
}
