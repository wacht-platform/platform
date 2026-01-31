use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, patch, post, put},
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::api;
use crate::middleware::backend_deployment_middleware;
use crate::middleware::deployment_access::deployment_access_middleware;
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

fn ai_context_routes() -> Router<AppState> {
    Router::new().route(
        "/ai/execution-context",
        get(api::ai_execution_context::get_execution_contexts)
            .post(api::ai_execution_context::create_execution_context),
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
        // AI Workflows
        .route(
            "/ai/workflows",
            get(api::ai_workflows::get_ai_workflows).post(api::ai_workflows::create_ai_workflow),
        )
        .route(
            "/ai/workflows/{workflow_id}",
            get(api::ai_workflows::get_ai_workflow_by_id)
                .patch(api::ai_workflows::update_ai_workflow)
                .delete(api::ai_workflows::delete_ai_workflow),
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
            "/ai/knowledge-bases/search",
            get(api::ai_knowledge_base_search::search_knowledge_base),
        )
        .route(
            "/ai/knowledge-bases/{kb_id}/search",
            get(api::ai_knowledge_base_search::search_specific_knowledge_base),
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
        // Email Templates
        .route(
            "/settings/email-templates/{template_name}",
            get(api::settings::get_deployment_email_template),
        )
        .route(
            "/settings/email-templates/{template_name}",
            patch(api::settings::update_deployment_email_template),
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
    Router::new()
        .route("/analytics/stats", get(api::analytics::get_analytics_stats))
        .route(
            "/analytics/recent-signups",
            get(api::analytics::get_recent_signups),
        )
        .route(
            "/analytics/recent-signins",
            get(api::analytics::get_recent_signins),
        )
}

// Utility Routes
fn utility_routes() -> Router<AppState> {
    Router::new()
        .route("/upload/{image_type}", post(api::upload::upload_image))
}

fn base_deployment_routes() -> Router<AppState> {
    user_management_routes()
        .merge(b2b_routes())
        .merge(settings_routes())
        .merge(segments_routes())
        .merge(analytics_routes())
        .merge(utility_routes())
}

fn billing_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/billing",
            get(api::billing::get_billing_account).patch(api::billing::update_billing_account),
        )
        .route("/billing/checkout", post(api::billing::create_checkout))
        .route("/billing/pulse/buy", post(api::billing::create_pulse_checkout))
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
            "/verify-dns",
            post(api::project::verify_deployment_dns_records),
        )
        .route(
            "/webhooks/status",
            get(api::webhook_console::get_webhook_status),
        )
        .route(
            "/webhooks/rotate-secret",
            post(api::webhook_console::rotate_webhook_secret),
        )
        .route(
            "/webhooks/events",
            get(api::webhook_console::get_available_events),
        )
        .route(
            "/webhooks/endpoints",
            get(api::webhook_console::list_webhook_endpoints),
        )
        .route(
            "/webhooks/endpoints",
            post(api::webhook_console::create_webhook_endpoint),
        )
        .route(
            "/webhooks/endpoints/{endpoint_id}",
            patch(api::webhook_console::update_webhook_endpoint),
        )
        .route(
            "/webhooks/endpoints/{endpoint_id}",
            delete(api::webhook_console::delete_webhook_endpoint),
        )
        .route(
            "/webhooks/analytics",
            get(api::webhook_console::get_webhook_analytics),
        )
        .route(
            "/webhooks/analytics/timeseries",
            get(api::webhook_console::get_webhook_timeseries),
        )
        .route(
            "/webhooks/deliveries",
            get(api::webhook_console::get_webhook_deliveries),
        )
        .route(
            "/webhooks/deliveries/{delivery_id}",
            get(api::webhook_console::get_webhook_delivery_details),
        )
        .route(
            "/webhooks/deliveries/replay",
            post(api::webhook_console::replay_webhook_deliveries),
        )
        .route(
            "/webhooks/endpoints/{endpoint_id}/reactivate",
            post(api::webhook_console::reactivate_webhook_endpoint),
        )
        .route(
            "/webhooks/endpoints/{endpoint_id}/test",
            post(api::webhook_console::test_webhook_endpoint),
        )
        .route(
            "/api-keys/status",
            get(api::api_key_console::get_api_key_status),
        )
        .route(
            "/api-keys/deactivate",
            post(api::api_key_console::deactivate_api_keys),
        )
        .route(
            "/api-keys/stats",
            get(api::api_key_console::get_api_key_stats),
        )
        .route("/api-keys", get(api::api_key_console::list_api_keys))
        .route("/api-keys", post(api::api_key_console::create_api_key))
        .route(
            "/api-keys/{key_id}",
            delete(api::api_key_console::revoke_api_key),
        )
        .route(
            "/api-keys/{key_id}/rotate",
            post(api::api_key_console::rotate_api_key),
        )
        .route(
            "/settings/email/smtp",
            post(api::settings::update_smtp_config)
                .delete(api::settings::remove_smtp_config),
        )
        .route(
            "/settings/email/smtp/verify",
            post(api::settings::verify_smtp_connection),
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
            "/webhooks/apps/{app_name}",
            patch(api::webhook::update_webhook_app)
                .get(api::webhook::get_webhook_app)
                .delete(api::webhook::delete_webhook_app),
        )
        .route(
            "/webhooks/apps/{app_name}/rotate-secret",
            post(api::webhook::rotate_webhook_secret),
        )
        .route(
            "/webhooks/apps/{app_name}/events",
            get(api::webhook::get_webhook_events),
        )
        .route(
            "/webhooks/apps/{app_name}/endpoints",
            get(api::webhook::list_webhook_endpoints),
        )
        .route(
            "/webhooks/apps/{app_name}/stats",
            get(api::webhook::get_webhook_stats),
        )
        .route(
            "/webhooks/apps/{app_name}/deliveries",
            get(api::webhook::get_app_webhook_deliveries),
        )
        .route(
            "/webhooks/endpoints",
            post(api::webhook::create_webhook_endpoint),
        )
        .route(
            "/webhooks/endpoints/{endpoint_id}",
            patch(api::webhook::update_webhook_endpoint),
        )
        .route(
            "/webhooks/endpoints/{endpoint_id}",
            delete(api::webhook::delete_webhook_endpoint),
        )
        .route(
            "/webhooks/apps/{app_name}/endpoints/{endpoint_id}/test",
            post(api::webhook::test_webhook_endpoint),
        )
        .route(
            "/webhooks/apps/{app_name}/trigger",
            post(api::webhook::trigger_webhook_event),
        )
        .route(
            "/webhooks/apps/{app_name}/trigger/batch",
            post(api::webhook::batch_trigger_webhook_events),
        )
        .route(
            "/webhooks/deliveries/{delivery_id}",
            get(api::webhook::get_webhook_delivery_details),
        )
        .route(
            "/webhooks/apps/{app_name}/deliveries/replay",
            post(api::webhook::replay_webhook_delivery),
        )
        .route(
            "/webhooks/endpoints/{endpoint_id}/reactivate",
            post(api::webhook::reactivate_webhook_endpoint),
        )
        .route(
            "/webhooks/apps/{app_name}/analytics",
            get(api::webhook::get_webhook_analytics),
        )
        .route(
            "/webhooks/apps/{app_name}/timeseries",
            get(api::webhook::get_webhook_timeseries),
        )
        .route("/api-keys/apps", get(api::api_key::list_api_key_apps))
        .route("/api-keys/apps", post(api::api_key::create_api_key_app))
        .route(
            "/api-keys/apps/{app_name}",
            get(api::api_key::get_api_key_app),
        )
        .route(
            "/api-keys/apps/{app_name}",
            patch(api::api_key::update_api_key_app),
        )
        .route(
            "/api-keys/apps/{app_name}",
            delete(api::api_key::delete_api_key_app),
        )
        .route(
            "/api-keys/apps/{app_name}/keys",
            get(api::api_key::list_api_keys),
        )
        .route(
            "/api-keys/apps/{app_name}/keys",
            post(api::api_key::create_api_key),
        )
        .route("/api-keys/revoke", post(api::api_key::revoke_api_key))
        .route("/api-keys/rotate", post(api::api_key::rotate_api_key))
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
        .merge(ai_context_routes())
        .merge(billing_routes())
        .nest("/deployments/{deployment_id}", deployment_routes)
        .layer(auth_layer);

    Router::new()
        .merge(health_routes())
        .merge(public_webhook_routes())
        .merge(protected_routes)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}

pub async fn create_backend_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let backend_routs = base_deployment_routes()
        .merge(ai_routes())
        .merge(backend_specific_routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            backend_deployment_middleware,
        ));

    Router::new()
        .merge(health_routes())
        .merge(backend_routs)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
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
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
