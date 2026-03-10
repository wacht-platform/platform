use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::Redirect,
};
use commands::{
    ConsumeOAuthAuthorizationCode, CreateOAuthClientCommand, DeactivateOAuthClient,
    EnqueueOAuthGrantLastUsed, IssueOAuthAuthorizationCode, IssueOAuthTokenPair,
    RevokeOAuthAccessTokenByHash, RevokeOAuthRefreshTokenByHash, RevokeOAuthRefreshTokenById,
    RevokeOAuthRefreshTokenFamily, RevokeOAuthTokensByGrant, SetOAuthClientRegistrationAccessToken,
    SetOAuthRefreshTokenReplacement, UpdateOAuthClientSettings,
    api_key_app::EnsureUserApiAuthAppCommand,
};
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::oauth_runtime::{
    OAuthAuthorizeInitiatedResponse, OAuthAuthorizeRequest, OAuthConsentSubmitRequest,
    OAuthDynamicClientRegistrationRequest, OAuthDynamicClientRegistrationResponse,
    OAuthDynamicClientUpdateRequest, OAuthIntrospectRequest, OAuthIntrospectResponse,
    OAuthProtectedResourceMetadataResponse, OAuthRegisterPathParams, OAuthRevokeRequest,
    OAuthRevokeResponse, OAuthServerMetadataResponse, OAuthTokenRequest, OAuthTokenResponse,
};
use models::api_key::OAuthScopeDefinition;
use models::error::AppError;
use queries::{
    GetRuntimeAuthorizationCodeForExchangeQuery, GetRuntimeDeploymentHostsByIdQuery,
    GetRuntimeIntrospectionDataQuery, GetRuntimeOAuthClientByClientIdQuery,
    GetRuntimeRefreshTokenForExchangeQuery, RuntimeOAuthAppData, RuntimeOAuthClientData,
};
use redis::AsyncCommands;

use crate::{
    api::oauth_runtime::{
        helpers::{
            append_oauth_redirect_params, authenticate_client, client_secret_expires_at_for_method,
            derive_shared_secret, ensure_or_create_grant_coverage,
            ensure_registration_access_token, generate_prefixed_token,
            generate_registration_access_token, hash_value, is_valid_granted_resource_indicator,
            is_valid_resource_indicator, oauth_consent_backend_base_url,
            oauth_consent_handoff_redis_key, parse_scope_string, resolve_issuer_from_oauth_app,
            resolve_oauth_app_from_host, sign_oauth_consent_request_token,
            validate_grant_and_entitlement, verify_oauth_consent_request_token, verify_pkce,
        },
        token_handlers::{
            OAuthEndpointError, map_token_app_error, map_token_auth_error, map_token_pkce_error,
            oauth_token_error,
        },
        types::GrantValidationResult,
        types::{OAuthConsentHandoffPayload, OAuthConsentRequestTokenClaims},
    },
    application::response::ApiErrorResponse,
};
use common::deps;

mod authorize;
mod metadata;
mod registration;
mod token;

pub use authorize::{oauth_authorize_get, oauth_consent_submit};
pub use metadata::{oauth_protected_resource_metadata, oauth_server_metadata};
pub use registration::{
    oauth_delete_registered_client, oauth_get_registered_client, oauth_register_client,
    oauth_update_registered_client,
};
pub use token::{oauth_introspect, oauth_revoke, oauth_token};

async fn resolve_oauth_app_and_issuer(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<(RuntimeOAuthAppData, String), ApiErrorResponse> {
    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    Ok((oauth_app, issuer))
}
