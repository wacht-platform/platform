use axum::{
    Router,
    routing::{get, post},
};
use common::state::AppState;

use crate::api;

pub async fn create_oauth_router(state: AppState) -> Router {
    super::apply_common_http_layers(
        Router::new()
            .route("/health", get(api::health::check))
            .route(
                "/.well-known/oauth-authorization-server",
                get(api::oauth_runtime::oauth_server_metadata),
            )
            .route(
                "/.well-known/oauth-protected-resource",
                get(api::oauth_runtime::oauth_protected_resource_metadata),
            )
            // ---- OIDC extension ----
            .route(
                "/.well-known/openid-configuration",
                get(api::oauth_runtime::openid_configuration),
            )
            .route(
                "/.well-known/jwks.json",
                get(api::oauth_runtime::jwks),
            )
            .route(
                "/oauth/userinfo",
                get(api::oauth_runtime::userinfo).post(api::oauth_runtime::userinfo),
            )
            .route(
                "/oauth/logout",
                get(api::oauth_runtime::oauth_logout)
                    .post(api::oauth_runtime::oauth_logout_post),
            )
            .route(
                "/oauth/authorize",
                get(api::oauth_runtime::oauth_authorize_get)
                    .post(api::oauth_runtime::oauth_authorize_post),
            )
            .route(
                "/oauth/consent/submit",
                post(api::oauth_runtime::oauth_consent_submit),
            )
            .route("/oauth/token", post(api::oauth_runtime::oauth_token))
            .route("/oauth/revoke", post(api::oauth_runtime::oauth_revoke))
            .route(
                "/oauth/introspect",
                post(api::oauth_runtime::oauth_introspect),
            )
            .route(
                "/oauth/register",
                post(api::oauth_runtime::oauth_register_client),
            )
            .route(
                "/oauth/register/{client_id}",
                get(api::oauth_runtime::oauth_get_registered_client)
                    .put(api::oauth_runtime::oauth_update_registered_client)
                    .delete(api::oauth_runtime::oauth_delete_registered_client),
            )
            .with_state(state),
    )
}
