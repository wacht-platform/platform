use axum::{
    Router,
    routing::{delete, get, patch, post},
};

use crate::api;
use common::state::AppState;

pub(super) fn api_auth_routes() -> Router<AppState> {
    Router::new()
        .route("/api-auth/apps", get(api::api_key::list_api_auth_apps))
        .route("/api-auth/apps", post(api::api_key::create_api_auth_app))
        .route(
            "/api-auth/apps/{app_slug}",
            get(api::api_key::get_api_auth_app),
        )
        .route(
            "/api-auth/apps/{app_slug}",
            patch(api::api_key::update_api_auth_app),
        )
        .route(
            "/api-auth/apps/{app_slug}",
            delete(api::api_key::delete_api_auth_app),
        )
        .route(
            "/api-auth/rate-limit-schemes",
            get(api::api_key::list_rate_limit_schemes).post(api::api_key::create_rate_limit_scheme),
        )
        .route(
            "/api-auth/rate-limit-schemes/{slug}",
            get(api::api_key::get_rate_limit_scheme)
                .patch(api::api_key::update_rate_limit_scheme)
                .delete(api::api_key::delete_rate_limit_scheme),
        )
        .route(
            "/api-auth/apps/{app_slug}/keys",
            get(api::api_key::list_api_keys),
        )
        .route(
            "/api-auth/apps/{app_slug}/keys",
            post(api::api_key::create_api_key),
        )
        .route(
            "/api-auth/apps/{app_slug}/keys/{key_id}/revoke",
            post(api::api_key::revoke_api_key_for_app),
        )
        .route(
            "/api-auth/apps/{app_slug}/keys/{key_id}/rotate",
            post(api::api_key::rotate_api_key_for_app),
        )
        .route(
            "/api-auth/apps/{app_slug}/audit/logs",
            get(api::api_key::get_api_audit_logs),
        )
        .route(
            "/api-auth/apps/{app_slug}/audit/analytics",
            get(api::api_key::get_api_audit_analytics),
        )
        .route(
            "/api-auth/apps/{app_slug}/audit/timeseries",
            get(api::api_key::get_api_audit_timeseries),
        )
        .route("/oauth/apps", get(api::oauth::list_oauth_apps))
        .route("/oauth/apps", post(api::oauth::create_oauth_app))
        .route(
            "/oauth/apps/{oauth_app_slug}",
            patch(api::oauth::update_oauth_app),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/verify-domain",
            post(api::oauth::verify_oauth_app_domain),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/scopes/{scope}",
            patch(api::oauth::update_oauth_scope),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/scopes/{scope}/archive",
            post(api::oauth::archive_oauth_scope),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/scopes/{scope}/unarchive",
            post(api::oauth::unarchive_oauth_scope),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/scopes/{scope}/mapping",
            post(api::oauth::set_oauth_scope_mapping),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/clients",
            get(api::oauth::list_oauth_clients),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/clients",
            post(api::oauth::create_oauth_client),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/clients/{oauth_client_id}",
            patch(api::oauth::update_oauth_client).delete(api::oauth::deactivate_oauth_client),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/clients/{oauth_client_id}/rotate-secret",
            post(api::oauth::rotate_oauth_client_secret),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/clients/{oauth_client_id}/grants",
            get(api::oauth::list_oauth_grants),
        )
        .route(
            "/oauth/apps/{oauth_app_slug}/clients/{oauth_client_id}/grants/{grant_id}/revoke",
            post(api::oauth::revoke_oauth_grant),
        )
}
