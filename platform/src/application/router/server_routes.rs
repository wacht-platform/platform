use axum::{
    Router,
    routing::{delete, get, patch, post},
};

use crate::api;
use common::state::AppState;

pub(super) fn billing_routes() -> Router<AppState> {
    Router::new()
        .route("/billing", get(api::billing::get_billing_account))
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

pub(super) fn console_specific_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/session/tickets",
            post(api::session_tickets::create_session_ticket),
        )
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

pub(super) fn backend_specific_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/session/tickets",
            post(api::session_tickets::create_backend_session_ticket),
        )
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
}
