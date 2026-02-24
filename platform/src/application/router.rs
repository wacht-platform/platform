use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::Request,
    routing::{delete, get, patch, post, put},
};
use std::time::Duration;
use tower_http::{
    classify::ServerErrorsFailureClass,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::api;
use crate::middleware::backend_deployment_middleware;
use crate::middleware::deployment_access::deployment_access_middleware;
use crate::middleware::platform_source::{
    mark_backend_platform_source, mark_console_platform_source,
};
use common::state::AppState;

fn health_routes() -> Router<AppState> {
    Router::new().route("/health", get(api::health::check))
}

fn public_webhook_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/webhooks/dodo",
            post(api::billing_webhook::handle_dodo_webhook),
        )
        .route(
            "/webhooks/prelude/{deployment_id}",
            post(api::prelude_webhook::handle_prelude_webhook),
        )
}

fn project_routes() -> Router<AppState> {
    Router::new()
        .route("/projects", get(api::project::get_projects))
        .route("/project", post(api::project::create_project))
        .route("/project/{id}", delete(api::project::delete_project))
        .route(
            "/project/{project_id}/staging-deployment",
            post(api::project::create_staging_deployment),
        )
        .route(
            "/project/{project_id}/production-deployment",
            post(api::project::create_production_deployment),
        )
        .route(
            "/project/{project_id}/deployment/{deployment_id}",
            delete(api::project::delete_deployment),
        )
}

fn ai_routes() -> Router<AppState> {
    Router::new()
        // AI Agents
        .route(
            "/ai/agents",
            get(api::ai_agents::get_ai_agents).post(api::ai_agents::create_ai_agent),
        )
        .route(
            "/ai/agents/{agent_id}",
            get(api::ai_agents::get_ai_agent_by_id)
                .patch(api::ai_agents::update_ai_agent)
                .delete(api::ai_agents::delete_ai_agent),
        )
        .route(
            "/ai/agents/{agent_id}/details",
            get(api::ai_agents::get_ai_agent_details),
        )
        .route(
            "/ai/agents/{agent_id}/sub-agents",
            get(api::ai_agents::get_agent_sub_agents),
        )
        .route(
            "/ai/agents/{agent_id}/sub-agents/{sub_agent_id}",
            post(api::ai_agents::attach_sub_agent_to_agent)
                .delete(api::ai_agents::detach_sub_agent_from_agent),
        )
        // AI Tools
        .route(
            "/ai/tools",
            get(api::ai_tools::get_ai_tools).post(api::ai_tools::create_ai_tool),
        )
        .route(
            "/ai/tools/{tool_id}",
            get(api::ai_tools::get_ai_tool_by_id)
                .patch(api::ai_tools::update_ai_tool)
                .delete(api::ai_tools::delete_ai_tool),
        )
        .route(
            "/ai/agents/{agent_id}/tools",
            get(api::ai_tools::get_agent_tools),
        )
        .route(
            "/ai/agents/{agent_id}/tools/{tool_id}",
            post(api::ai_tools::attach_tool_to_agent).delete(api::ai_tools::detach_tool_from_agent),
        )
        // AI Knowledge Bases
        .route(
            "/ai/knowledge-bases",
            get(api::ai_knowledge_base::get_ai_knowledge_bases)
                .post(api::ai_knowledge_base::create_ai_knowledge_base),
        )
        .route(
            "/ai/knowledge-bases/{kb_id}",
            get(api::ai_knowledge_base::get_ai_knowledge_base_by_id)
                .patch(api::ai_knowledge_base::update_ai_knowledge_base)
                .delete(api::ai_knowledge_base::delete_ai_knowledge_base),
        )
        .route(
            "/ai/knowledge-bases/{kb_id}/documents",
            get(api::ai_knowledge_base::get_knowledge_base_documents)
                .post(api::ai_knowledge_base::upload_knowledge_base_document)
                .layer(DefaultBodyLimit::max(25 * 1024 * 1024)),
        )
        .route(
            "/ai/knowledge-bases/{kb_id}/documents/{document_id}",
            delete(api::ai_knowledge_base::delete_knowledge_base_document),
        )
        .route(
            "/ai/agents/{agent_id}/knowledge-bases",
            get(api::ai_knowledge_base::get_agent_knowledge_bases),
        )
        .route(
            "/ai/agents/{agent_id}/knowledge-bases/{kb_id}",
            post(api::ai_knowledge_base::attach_knowledge_base_to_agent)
                .delete(api::ai_knowledge_base::detach_knowledge_base_from_agent),
        )
        // Agent Integrations
        .route(
            "/ai/agents/{agent_id}/integrations",
            get(api::agent_integrations::get_agent_integrations)
                .post(api::agent_integrations::create_agent_integration),
        )
        .route(
            "/ai/agents/{agent_id}/integrations/{integration_id}",
            get(api::agent_integrations::get_agent_integration_by_id)
                .patch(api::agent_integrations::update_agent_integration)
                .delete(api::agent_integrations::delete_agent_integration),
        )
        // MCP Servers
        .route(
            "/ai/mcp-servers",
            get(api::mcp_servers::get_mcp_servers).post(api::mcp_servers::create_mcp_server),
        )
        .route(
            "/ai/mcp-servers/discover",
            post(api::mcp_servers::discover_mcp_server_auth),
        )
        .route(
            "/ai/mcp-servers/{mcp_server_id}",
            get(api::mcp_servers::get_mcp_server_by_id)
                .patch(api::mcp_servers::update_mcp_server)
                .delete(api::mcp_servers::delete_mcp_server),
        )
        .route(
            "/ai/agents/{agent_id}/mcp-servers",
            get(api::mcp_servers::get_agent_mcp_servers),
        )
        .route(
            "/ai/agents/{agent_id}/mcp-servers/{mcp_server_id}",
            post(api::mcp_servers::attach_mcp_server_to_agent)
                .delete(api::mcp_servers::detach_mcp_server_from_agent),
        )
        // AI Settings
        .route(
            "/ai/settings",
            get(api::ai_settings::get_ai_settings).put(api::ai_settings::update_ai_settings),
        )
}

// User Management Routes
fn user_management_routes() -> Router<AppState> {
    Router::new()
        // Users
        .route("/users", get(api::user::get_active_user_list))
        .route("/users", post(api::user::create_user))
        .route("/users/{user_id}/details", get(api::user::get_user_details))
        .route("/users/{user_id}", patch(api::user::update_user))
        .route("/users/{user_id}", delete(api::user::delete_user))
        .route(
            "/users/{user_id}/password",
            patch(api::user::update_user_password),
        )
        .route("/users/{user_id}/emails", post(api::user::add_user_email))
        .route(
            "/users/{user_id}/emails/{email_id}",
            patch(api::user::update_user_email),
        )
        .route(
            "/users/{user_id}/emails/{email_id}",
            delete(api::user::delete_user_email),
        )
        .route("/users/{user_id}/phones", post(api::user::add_user_phone))
        .route(
            "/users/{user_id}/phones/{phone_id}",
            patch(api::user::update_user_phone),
        )
        .route(
            "/users/{user_id}/phones/{phone_id}",
            delete(api::user::delete_user_phone),
        )
        .route(
            "/users/{user_id}/social-connections/{connection_id}",
            delete(api::user::delete_user_social_connection),
        )
        // Invitations
        .route(
            "/invitations",
            get(api::user::get_invited_user_list).post(api::user::invite_user),
        )
        .route(
            "/invitations/{invitation_id}",
            delete(api::user::delete_invitation),
        )
        // Waitlist
        .route("/waitlist", get(api::user::get_user_waitlist))
        .route(
            "/waitlist/{waitlist_user_id}/approve",
            post(api::user::approve_waitlist_user),
        )
        // Session
        .route(
            "/session/tickets",
            post(api::session_tickets::create_session_ticket),
        )
}

// B2B Routes
fn b2b_routes() -> Router<AppState> {
    Router::new()
        // Workspaces
        .route("/workspaces", get(api::b2b::get_workspace_list))
        .route(
            "/workspaces/{workspace_id}",
            get(api::b2b::get_workspace_details)
                .patch(api::b2b::update_workspace)
                .delete(api::b2b::delete_workspace),
        )
        .route(
            "/workspaces/roles",
            get(api::b2b::get_deployment_workspace_roles),
        )
        .route(
            "/workspaces/{workspace_id}/roles",
            post(api::b2b::create_workspace_role),
        )
        .route(
            "/workspaces/{workspace_id}/roles/{role_id}",
            patch(api::b2b::update_workspace_role).delete(api::b2b::delete_workspace_role),
        )
        .route(
            "/workspaces/{workspace_id}/members",
            get(api::b2b::get_workspace_members).post(api::b2b::add_workspace_member),
        )
        .route(
            "/workspaces/{workspace_id}/members/{membership_id}",
            delete(api::b2b::remove_workspace_member).patch(api::b2b::update_workspace_member),
        )
        // Organizations
        .route(
            "/organizations",
            get(api::b2b::get_organization_list).post(api::b2b::create_organization),
        )
        .route(
            "/organizations/{organization_id}",
            get(api::b2b::get_organization_details)
                .patch(api::b2b::update_organization)
                .delete(api::b2b::delete_organization),
        )
        .route(
            "/organizations/{organization_id}/workspaces",
            post(api::b2b::create_workspace_for_organization),
        )
        .route(
            "/organizations/{organization_id}/members",
            get(api::b2b::get_organization_members).post(api::b2b::add_organization_member),
        )
        .route(
            "/organizations/{organization_id}/members/{membership_id}",
            delete(api::b2b::remove_organization_member)
                .patch(api::b2b::update_organization_member),
        )
        .route(
            "/organizations/{organization_id}/roles",
            post(api::b2b::create_organization_role),
        )
        .route(
            "/organizations/{organization_id}/roles/{role_id}",
            patch(api::b2b::update_organization_role).delete(api::b2b::delete_organization_role),
        )
        .route(
            "/organizations/roles",
            get(api::b2b::get_deployment_org_roles),
        )
}

// Settings Routes
fn settings_routes() -> Router<AppState> {
    Router::new()
        // Deployment info
        .route("/", get(api::settings::get_deployment_with_settings))
        // JWT Templates (configuration)
        .route(
            "/jwt-templates",
            get(api::settings::get_deployment_jwt_templates),
        )
        .route(
            "/jwt-templates",
            post(api::settings::create_deployment_jwt_template),
        )
        .route(
            "/jwt-templates/{id}",
            patch(api::settings::update_deployment_jwt_template),
        )
        .route(
            "/jwt-templates/{id}",
            delete(api::settings::delete_deployment_jwt_template),
        )
        // Settings
        .route(
            "/settings/auth",
            patch(api::settings::update_deployment_auth_settings),
        )
        .route(
            "/settings/display",
            patch(api::settings::update_deployment_display_settings),
        )
        .route(
            "/settings/restrictions",
            patch(api::settings::update_deployment_restrictions),
        )
        .route(
            "/settings/b2b",
            patch(api::b2b::update_deployment_b2b_settings),
        )
        // Social Connections
        .route(
            "/settings/social-connections",
            get(api::connection::get_deployment_social_connections),
        )
        .route(
            "/settings/social-connections",
            put(api::connection::upsert_deployment_social_connection),
        )
        // SMTP Configuration
        .route(
            "/settings/email/smtp",
            post(api::settings::update_smtp_config).delete(api::settings::remove_smtp_config),
        )
        .route(
            "/settings/email/smtp/verify",
            post(api::settings::verify_smtp_connection),
        )
        // Email Templates
        .route(
            "/settings/email-templates/{template_name}",
            get(api::settings::get_deployment_email_template),
        )
        .route(
            "/settings/email-templates/{template_name}",
            patch(api::settings::update_deployment_email_template),
        )
        // Image Upload
        .route(
            "/settings/upload/{image_type}",
            post(api::upload::upload_image),
        )
}

// Segments Routes
fn segments_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/segments",
            get(api::segments::list_segments).post(api::segments::create_segment),
        )
        .route("/segments/data", post(api::segments::get_segment_data))
        .route(
            "/segments/{id}",
            patch(api::segments::update_segment).delete(api::segments::delete_segment),
        )
        .route("/segments/{id}/assign", post(api::segments::assign_segment))
        .route("/segments/{id}/remove", post(api::segments::remove_segment))
}

// Analytics Routes
fn analytics_routes() -> Router<AppState> {
    Router::new().route("/analytics", get(api::analytics::get_analytics_stats))
}

fn base_deployment_routes() -> Router<AppState> {
    user_management_routes()
        .merge(b2b_routes())
        .merge(settings_routes())
        .merge(segments_routes())
        .merge(analytics_routes())
        .merge(api_auth_routes())
}

fn api_auth_routes() -> Router<AppState> {
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

fn billing_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/billing",
            get(api::billing::get_billing_account).patch(api::billing::update_billing_account),
        )
        .route("/billing/checkout", post(api::billing::create_checkout))
        .route(
            "/billing/pulse/buy",
            post(api::billing::create_pulse_checkout),
        )
        .route("/billing/portal", get(api::billing::get_portal_url))
        .route("/billing/usage", get(api::billing::get_current_usage))
        .route("/billing/invoices", get(api::billing::list_invoices))
        .route("/billing/change-plan", post(api::billing::change_plan))
        .route(
            "/billing/pulse/transactions",
            get(api::billing::list_pulse_transactions),
        )
}

fn console_specific_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/webhooks/event-catalogs",
            get(api::webhook::list_event_catalogs).post(api::webhook::create_event_catalog),
        )
        .route(
            "/webhooks/event-catalogs/{slug}",
            get(api::webhook::get_event_catalog).put(api::webhook::update_event_catalog),
        )
        .route(
            "/webhooks/event-catalogs/{slug}/append-events",
            post(api::webhook::append_events_to_catalog),
        )
        .route(
            "/webhooks/event-catalogs/{slug}/archive-event",
            post(api::webhook::archive_event_in_catalog),
        )
        .route(
            "/verify-dns",
            post(api::project::verify_deployment_dns_records),
        )
        .route(
            "/organizations/{org_id}/domains",
            post(api::enterprise_sso::create_domain_handler)
                .get(api::enterprise_sso::list_domains_handler),
        )
        .route(
            "/organizations/{org_id}/domains/{domain_id}",
            delete(api::enterprise_sso::delete_domain_handler),
        )
        .route(
            "/organizations/{org_id}/domains/{domain_id}/verify",
            post(api::enterprise_sso::verify_domain_handler),
        )
        .route(
            "/organizations/{org_id}/connections",
            post(api::enterprise_sso::create_connection_handler)
                .get(api::enterprise_sso::list_connections_handler),
        )
        .route(
            "/organizations/{org_id}/connections/{connection_id}",
            post(api::enterprise_sso::update_connection_handler)
                .delete(api::enterprise_sso::delete_connection_handler),
        )
        .route(
            "/organizations/{org_id}/connections/{connection_id}/scim-token",
            get(api::enterprise_sso::get_scim_token_handler)
                .post(api::enterprise_sso::generate_scim_token_handler)
                .delete(api::enterprise_sso::revoke_scim_token_handler),
        )
}

fn backend_specific_routes() -> Router<AppState> {
    Router::new()
        .route("/webhooks/apps", get(api::webhook::list_webhook_apps))
        .route("/webhooks/apps", post(api::webhook::create_webhook_app))
        .route(
            "/webhooks/event-catalogs",
            get(api::webhook::list_event_catalogs).post(api::webhook::create_event_catalog),
        )
        .route(
            "/webhooks/event-catalogs/{slug}",
            get(api::webhook::get_event_catalog).put(api::webhook::update_event_catalog),
        )
        .route(
            "/webhooks/event-catalogs/{slug}/append-events",
            post(api::webhook::append_events_to_catalog),
        )
        .route(
            "/webhooks/event-catalogs/{slug}/archive-event",
            post(api::webhook::archive_event_in_catalog),
        )
        .route(
            "/webhooks/apps/{app_slug}",
            patch(api::webhook::update_webhook_app)
                .get(api::webhook::get_webhook_app)
                .delete(api::webhook::delete_webhook_app),
        )
        .route(
            "/webhooks/apps/{app_slug}/rotate-secret",
            post(api::webhook::rotate_webhook_secret),
        )
        .route(
            "/webhooks/apps/{app_slug}/events",
            get(api::webhook::get_webhook_events),
        )
        .route(
            "/webhooks/apps/{app_slug}/catalog",
            get(api::webhook::get_webhook_catalog),
        )
        .route(
            "/webhooks/apps/{app_slug}/endpoints",
            get(api::webhook::list_webhook_endpoints),
        )
        .route(
            "/webhooks/apps/{app_slug}/endpoints",
            post(api::webhook::create_webhook_endpoint_for_app),
        )
        .route(
            "/webhooks/apps/{app_slug}/endpoints/{endpoint_id}",
            patch(api::webhook::update_webhook_endpoint_for_app),
        )
        .route(
            "/webhooks/apps/{app_slug}/endpoints/{endpoint_id}",
            delete(api::webhook::delete_webhook_endpoint_for_app),
        )
        .route(
            "/webhooks/apps/{app_slug}/stats",
            get(api::webhook::get_webhook_stats),
        )
        .route(
            "/webhooks/apps/{app_slug}/deliveries",
            get(api::webhook::get_app_webhook_deliveries),
        )
        .route(
            "/webhooks/apps/{app_slug}/endpoints/{endpoint_id}/test",
            post(api::webhook::test_webhook_endpoint),
        )
        .route(
            "/webhooks/apps/{app_slug}/trigger",
            post(api::webhook::trigger_webhook_event),
        )
        .route(
            "/webhooks/apps/{app_slug}/deliveries/{delivery_id}",
            get(api::webhook::get_webhook_delivery_details_for_app),
        )
        .route(
            "/webhooks/apps/{app_slug}/deliveries/replay",
            get(api::webhook::list_webhook_replay_tasks)
                .post(api::webhook::replay_webhook_delivery),
        )
        .route(
            "/webhooks/apps/{app_slug}/deliveries/replay/{task_id}",
            get(api::webhook::get_webhook_replay_task_status),
        )
        .route(
            "/webhooks/apps/{app_slug}/deliveries/replay/{task_id}/cancel",
            post(api::webhook::cancel_webhook_replay_task),
        )
        .route(
            "/webhooks/endpoints/{endpoint_id}/reactivate",
            post(api::webhook::reactivate_webhook_endpoint),
        )
        .route(
            "/webhooks/apps/{app_slug}/analytics",
            get(api::webhook::get_webhook_analytics),
        )
        .route(
            "/webhooks/apps/{app_slug}/timeseries",
            get(api::webhook::get_webhook_timeseries),
        )
        .route(
            "/notifications",
            post(api::notifications::create_notification),
        )
        .route(
            "/ai/execution-contexts",
            get(api::ai_execution_context::get_execution_contexts_backend)
                .post(api::ai_execution_context::create_execution_context_backend),
        )
        .route(
            "/ai/execution-contexts/{context_id}",
            patch(api::ai_execution_context::update_execution_context),
        )
        .route(
            "/ai/execution-contexts/{context_id}/execute",
            post(api::ai_execution_context::execute_agent_async),
        )
}

pub async fn create_console_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    wacht::init_from_env().await.unwrap();

    use wacht::middleware::AuthLayer;
    let auth_layer = AuthLayer::new();

    let deployment_routes = base_deployment_routes()
        .merge(ai_routes())
        .merge(console_specific_routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            deployment_access_middleware,
        ));

    let protected_routes = Router::new()
        .merge(project_routes())
        .merge(billing_routes())
        .nest("/deployments/{deployment_id}", deployment_routes)
        .layer(auth_layer);

    Router::new()
        .merge(health_routes())
        .merge(public_webhook_routes())
        .merge(protected_routes)
        .with_state(state)
        .layer(axum::middleware::from_fn(mark_console_platform_source))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                    )
                })
                .on_failure(
                    |error: ServerErrorsFailureClass, _latency: Duration, _span: &tracing::Span| {
                        tracing::error!(
                            status = %error,
                            latency_ms = _latency.as_millis(),
                            "response failed"
                        );
                    },
                ),
        )
        .layer(cors)
}

pub async fn create_backend_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let backend_routes = base_deployment_routes()
        .merge(ai_routes())
        .merge(backend_specific_routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            backend_deployment_middleware,
        ));

    Router::new()
        .merge(health_routes())
        .merge(backend_routes)
        .with_state(state)
        .layer(axum::middleware::from_fn(mark_backend_platform_source))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                    )
                })
                .on_failure(
                    |error: ServerErrorsFailureClass, _latency: Duration, _span: &tracing::Span| {
                        tracing::error!(
                            status = %error,
                            latency_ms = _latency.as_millis(),
                            "response failed"
                        );
                    },
                ),
        )
        .layer(cors)
}

pub async fn create_frontend_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .merge(health_routes())
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                    )
                })
                .on_failure(
                    |error: ServerErrorsFailureClass, _latency: Duration, _span: &tracing::Span| {
                        tracing::error!(
                            status = %error,
                            latency_ms = _latency.as_millis(),
                            "response failed"
                        );
                    },
                ),
        )
        .layer(cors)
}

pub async fn create_oauth_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(api::health::check))
        .route(
            "/.well-known/oauth-authorization-server",
            get(api::oauth_runtime::oauth_server_metadata),
        )
        .route(
            "/oauth/authorize",
            get(api::oauth_runtime::oauth_authorize_get),
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
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                    )
                })
                .on_failure(
                    |error: ServerErrorsFailureClass, _latency: Duration, _span: &tracing::Span| {
                        tracing::error!(
                            status = %error,
                            latency_ms = _latency.as_millis(),
                            "response failed"
                        );
                    },
                ),
        )
        .layer(cors)
}
