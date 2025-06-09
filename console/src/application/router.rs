use axum::{
    Router,
    routing::{delete, get, patch, post, put},
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use super::HttpState;
use crate::api;

fn health_routes() -> Router<HttpState> {
    Router::new().route("/health", get(api::health::check))
}

fn project_routes() -> Router<HttpState> {
    Router::new()
        .route("/projects", get(api::project::get_projects))
        .route("/project", post(api::project::create_project))
        .route("/project/{id}", delete(api::project::delete_project))
        .route(
            "/project/{project_id}/production-deployment",
            post(api::project::create_production_deployment),
        )
        .route(
            "/project/{project_id}/deployment/{deployment_id}",
            delete(api::project::delete_deployment),
        )
        .route(
            "/deployment/{deployment_id}/verify-dns",
            post(api::project::verify_deployment_dns_records),
        )
}

fn deployment_routes() -> Router<HttpState> {
    let routes = Router::new()
        .route("/users", get(api::deployment::user::get_active_user_list))
        .route("/users", post(api::deployment::user::create_user))
        .route(
            "/users/{user_id}/details",
            get(api::deployment::user::get_user_details),
        )
        .route(
            "/users/{user_id}",
            patch(api::deployment::user::update_user),
        )
        .route(
            "/users/{user_id}/password",
            patch(api::deployment::user::update_user_password),
        )
        .route(
            "/users/{user_id}/emails",
            post(api::deployment::user::add_user_email),
        )
        .route(
            "/users/{user_id}/emails/{email_id}",
            patch(api::deployment::user::update_user_email),
        )
        .route(
            "/users/{user_id}/emails/{email_id}",
            delete(api::deployment::user::delete_user_email),
        )
        .route(
            "/users/{user_id}/phones",
            post(api::deployment::user::add_user_phone),
        )
        .route(
            "/users/{user_id}/phones/{phone_id}",
            patch(api::deployment::user::update_user_phone),
        )
        .route(
            "/users/{user_id}/phones/{phone_id}",
            delete(api::deployment::user::delete_user_phone),
        )
        .route(
            "/users/{user_id}/social-connections/{connection_id}",
            delete(api::deployment::user::delete_user_social_connection),
        )
        .route(
            "/invited-users",
            get(api::deployment::user::get_invited_user_list),
        )
        .route("/invited-users", post(api::deployment::user::invite_user))
        .route(
            "/user-waitlist",
            get(api::deployment::user::get_user_waitlist),
        )
        .route(
            "/user-waitlist/{waitlist_user_id}/approve",
            post(api::deployment::user::approve_waitlist_user),
        )
        .route(
            "/",
            get(api::deployment::settings::get_deployment_with_settings),
        )
        .route(
            "/jwt-templates",
            get(api::deployment::settings::get_deployment_jwt_templates),
        )
        .route(
            "/jwt-templates",
            post(api::deployment::settings::create_deployment_jwt_template),
        )
        .route(
            "/jwt-templates/{id}",
            patch(api::deployment::settings::update_deployment_jwt_template),
        )
        .route(
            "/jwt-templates/{id}",
            delete(api::deployment::settings::delete_deployment_jwt_template),
        )
        .route("/workspaces", get(api::deployment::b2b::get_workspace_list))
        .route(
            "/workspaces/{workspace_id}",
            get(api::deployment::b2b::get_workspace_details)
                .patch(api::deployment::b2b::update_workspace)
                .delete(api::deployment::b2b::delete_workspace),
        )
        .route(
            "/workspace-roles",
            get(api::deployment::b2b::get_deployment_workspace_roles),
        )
        .route(
            "/organizations",
            get(api::deployment::b2b::get_organization_list)
                .post(api::deployment::b2b::create_organization),
        )
        .route(
            "/organizations/{organization_id}",
            get(api::deployment::b2b::get_organization_details)
                .patch(api::deployment::b2b::update_organization)
                .delete(api::deployment::b2b::delete_organization),
        )
        .route(
            "/organizations/{organization_id}/workspaces",
            post(api::deployment::b2b::create_workspace_for_organization),
        )
        .route(
            "/workspaces/{workspace_id}/roles",
            post(api::deployment::b2b::create_workspace_role),
        )
        .route(
            "/workspaces/{workspace_id}/roles/{role_id}",
            patch(api::deployment::b2b::update_workspace_role)
                .delete(api::deployment::b2b::delete_workspace_role),
        )
        .route(
            "/organizations/{organization_id}/members",
            post(api::deployment::b2b::add_organization_member),
        )
        .route(
            "/organizations/{organization_id}/members/{membership_id}",
            patch(api::deployment::b2b::update_organization_member)
                .delete(api::deployment::b2b::remove_organization_member),
        )
        .route(
            "/organizations/{organization_id}/roles",
            post(api::deployment::b2b::create_organization_role),
        )
        .route(
            "/organizations/{organization_id}/roles/{role_id}",
            patch(api::deployment::b2b::update_organization_role)
                .delete(api::deployment::b2b::delete_organization_role),
        )
        .route(
            "/organization-roles",
            get(api::deployment::b2b::get_deployment_org_roles),
        )
        .route(
            "/settings/auth-settings",
            patch(api::deployment::settings::update_deployment_authetication_settings),
        )
        .route(
            "/settings/display-settings",
            patch(api::deployment::settings::update_deployment_ui_settings),
        )
        .route(
            "/restrictions",
            patch(api::deployment::settings::update_deployment_restrictions),
        )
        .route(
            "/social-connections",
            get(api::deployment::connection::get_deployment_social_connections),
        )
        .route(
            "/social-connections",
            put(api::deployment::connection::upsert_deployment_social_connection),
        )
        .route(
            "/settings/b2b-settings",
            patch(api::deployment::b2b::update_deployment_b2b_settings),
        )
        .route(
            "/email-templates/{template_name}",
            get(api::deployment::settings::get_email_template),
        )
        .route(
            "/email-templates/{template_name}",
            patch(api::deployment::settings::update_email_template),
        )
        .route(
            "/upload/{image_type}",
            post(api::deployment::upload::upload_image),
        );

    Router::new().nest("/deployments/{deployment_id}", routes)
}

fn ai_routes() -> Router<HttpState> {
    Router::new()
        // AI Agents
        .route(
            "/deployment/{deployment_id}/ai-agents",
            get(api::deployment::ai_agents::get_ai_agents)
                .post(api::deployment::ai_agents::create_ai_agent),
        )
        .route(
            "/deployment/{deployment_id}/ai-agents/{agent_id}",
            get(api::deployment::ai_agents::get_ai_agent_by_id)
                .patch(api::deployment::ai_agents::update_ai_agent)
                .delete(api::deployment::ai_agents::delete_ai_agent),
        )
        // AI Workflows
        .route(
            "/deployment/{deployment_id}/ai-workflows",
            get(api::deployment::ai_workflows::get_ai_workflows)
                .post(api::deployment::ai_workflows::create_ai_workflow),
        )
        .route(
            "/deployment/{deployment_id}/ai-workflows/{workflow_id}",
            get(api::deployment::ai_workflows::get_ai_workflow_by_id)
                .patch(api::deployment::ai_workflows::update_ai_workflow)
                .delete(api::deployment::ai_workflows::delete_ai_workflow),
        )
        .route(
            "/deployment/{deployment_id}/ai-tools",
            get(api::deployment::ai_tools::get_ai_tools)
                .post(api::deployment::ai_tools::create_ai_tool),
        )
        .route(
            "/deployment/{deployment_id}/ai-tools/{tool_id}",
            get(api::deployment::ai_tools::get_ai_tool_by_id)
                .patch(api::deployment::ai_tools::update_ai_tool)
                .delete(api::deployment::ai_tools::delete_ai_tool),
        )
        .route(
            "/deployment/{deployment_id}/ai-knowledge-bases",
            get(api::deployment::ai_knowledge_base::get_ai_knowledge_bases)
                .post(api::deployment::ai_knowledge_base::create_ai_knowledge_base),
        )
        .route(
            "/deployment/{deployment_id}/ai-knowledge-bases/{kb_id}",
            get(api::deployment::ai_knowledge_base::get_ai_knowledge_base_by_id)
                .patch(api::deployment::ai_knowledge_base::update_ai_knowledge_base)
                .delete(api::deployment::ai_knowledge_base::delete_ai_knowledge_base),
        )
        .route(
            "/deployment/{deployment_id}/ai-knowledge-bases/{kb_id}/documents",
            get(api::deployment::ai_knowledge_base::get_knowledge_base_documents)
                .post(api::deployment::ai_knowledge_base::upload_knowledge_base_document),
        )
        .route(
            "/deployment/{deployment_id}/ai-knowledge-bases/{kb_id}/documents/url",
            post(api::deployment::ai_knowledge_base::upload_knowledge_base_url),
        )
        .route(
            "/deployment/{deployment_id}/ai-knowledge-bases/{kb_id}/documents/{document_id}",
            delete(api::deployment::ai_knowledge_base::delete_knowledge_base_document),
        )
        // AI Knowledge Base Search
        .route(
            "/deployment/{deployment_id}/ai-knowledge-bases/search",
            get(api::deployment::ai_knowledge_base_search::search_knowledge_base),
        )
        .route(
            "/deployment/{deployment_id}/ai-knowledge-bases/{kb_id}/search",
            get(api::deployment::ai_knowledge_base_search::search_specific_knowledge_base),
        )
}

fn configure_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

pub fn create_router(state: HttpState) -> Router {
    let cors = configure_cors();

    Router::new()
        .merge(health_routes())
        .merge(project_routes())
        .merge(deployment_routes())
        .merge(ai_routes())
        .merge(api::analytics::analytics_routes())
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
