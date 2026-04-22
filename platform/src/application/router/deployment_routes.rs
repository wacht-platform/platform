use axum::{
    Router,
    routing::{delete, get, patch, post, put},
};

use crate::api;
use common::state::AppState;

pub(super) fn base_deployment_routes() -> Router<AppState> {
    user_management_routes()
        .merge(b2b_routes())
        .merge(settings_routes())
        .merge(segments_routes())
        .merge(analytics_routes())
        .merge(super::api_auth_routes::api_auth_routes())
}

fn user_management_routes() -> Router<AppState> {
    Router::new()
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
        .route(
            "/invitations",
            get(api::user::get_invited_user_list).post(api::user::invite_user),
        )
        .route(
            "/invitations/{invitation_id}",
            delete(api::user::delete_invitation),
        )
        .route("/waitlist", get(api::user::get_user_waitlist))
        .route(
            "/waitlist/{waitlist_user_id}/approve",
            post(api::user::approve_waitlist_user),
        )
}

fn b2b_routes() -> Router<AppState> {
    Router::new()
        .route("/workspaces", get(api::b2b::get_workspace_list))
        .route(
            "/workspaces/{workspace_id}",
            get(api::b2b::get_workspace_details)
                .patch(api::b2b::update_workspace)
                .delete(api::b2b::delete_workspace),
        )
        .route(
            "/workspaces/{workspace_id}/roles",
            get(api::b2b::get_workspace_roles).post(api::b2b::create_workspace_role),
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
            get(api::b2b::get_organization_roles).post(api::b2b::create_organization_role),
        )
        .route(
            "/organizations/{organization_id}/roles/{role_id}",
            patch(api::b2b::update_organization_role).delete(api::b2b::delete_organization_role),
        )
}

fn settings_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(api::settings::get_deployment_with_settings))
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
        .route(
            "/settings/social-connections",
            get(api::connection::get_deployment_social_connections),
        )
        .route(
            "/settings/social-connections",
            put(api::connection::upsert_deployment_social_connection),
        )
        .route(
            "/settings/email/smtp",
            post(api::settings::update_smtp_config).delete(api::settings::remove_smtp_config),
        )
        .route(
            "/settings/email/smtp/verify",
            post(api::settings::verify_smtp_connection),
        )
        .route(
            "/settings/email-templates/{template_name}",
            get(api::settings::get_deployment_email_template),
        )
        .route(
            "/settings/email-templates/{template_name}",
            patch(api::settings::update_deployment_email_template),
        )
        .route(
            "/settings/upload/{image_type}",
            post(api::upload::upload_image),
        )
}

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

fn analytics_routes() -> Router<AppState> {
    Router::new().route("/analytics", get(api::analytics::get_analytics_stats))
}
