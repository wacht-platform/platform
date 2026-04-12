use crate::application::response::ApiErrorResponse;
use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use models::plan_features::PlanFeature;
use queries::plan_access::CheckDeploymentFeatureAccessQuery;

/// Middleware function to check feature access
/// Use this to protect routes that require specific plan features
pub async fn require_feature(
    feature: PlanFeature,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
+ Clone {
    move |req: Request, next: Next| {
        let feature = feature;
        Box::pin(async move {
            // Extract deployment_id from request extensions
            let deployment_id = req.extensions().get::<i64>().copied().unwrap_or_default();

            if deployment_id == 0 {
                return ApiErrorResponse::internal("Deployment context not found").into_response();
            }

            // Get app state from extensions
            let state = match req.extensions().get::<common::state::AppState>() {
                Some(s) => s.clone(),
                None => {
                    return ApiErrorResponse::internal("App state not found").into_response();
                }
            };

            // Check if deployment has access to the feature
            let has_access = CheckDeploymentFeatureAccessQuery::new(deployment_id, feature)
                .execute_with_db(state.db_router.writer())
                .await
                .unwrap_or(false);

            if !has_access {
                return ApiErrorResponse::forbidden(format!(
                    "This feature requires a plan upgrade. Feature: {:?}",
                    feature
                ))
                .into_response();
            }

            next.run(req).await
        })
    }
}

/// Helper function to check feature access in handlers
/// Use this when you can't use middleware (e.g., in existing handlers)
pub async fn check_feature_access(
    deployment_id: i64,
    feature: PlanFeature,
    state: &common::state::AppState,
) -> Result<(), ApiErrorResponse> {
    let has_access = CheckDeploymentFeatureAccessQuery::new(deployment_id, feature)
        .execute_with_db(state.db_router.writer())
        .await
        .map_err(|e| {
            ApiErrorResponse::internal(format!("Failed to check feature access: {}", e))
        })?;

    if !has_access {
        return Err(ApiErrorResponse::forbidden(format!(
            "This feature requires a plan upgrade. Feature: {:?}",
            feature
        )));
    }

    Ok(())
}
