use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use chrono::{Datelike, Utc};
use models::webhook_analytics::{WebhookAnalyticsResult, WebhookTimeseriesResult};
use queries::GetWebhookAppByNameQuery;
use queries::webhook_analytics::{GetWebhookAnalyticsQuery, GetWebhookTimeseriesQuery};
use redis::{AsyncCommands, Script};

use crate::application::response::{ApiError, ApiErrorResponse, ApiResult, PaginatedResponse};
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    webhook_app::{
        CreateWebhookAppCommand, DeleteWebhookAppCommand, RotateWebhookSecretCommand,
        UpdateWebhookAppCommand,
    },
    webhook_endpoint::{
        CreateWebhookEndpointCommand, DeleteWebhookEndpointCommand, ReactivateEndpointCommand,
        TestWebhookEndpointCommand, UpdateWebhookEndpointCommand,
    },
    webhook_event_catalog::{CreateEventCatalogCommand, UpdateEventCatalogCommand},
    webhook_trigger::TriggerWebhookEventCommand,
};
use common::state::AppState;
use dto::clickhouse::webhook::{WebhookDeliveryListResponse, WebhookLog};
use dto::json::webhook_requests::{WebhookEndpoint as WebhookEndpointDto, *};
use models::webhook::{WebhookApp, WebhookEndpoint};
use queries::{
    Query as QueryTrait,
    webhook::{
        GetWebhookAppsQuery, GetWebhookEndpointsWithSubscriptionsQuery, GetWebhookEventsQuery,
    },
};

mod analytics;
mod apps;
mod deliveries;
mod dispatch;
mod endpoints;
mod replay;

pub use analytics::{get_webhook_analytics, get_webhook_timeseries};
pub use apps::{
    append_events_to_catalog, archive_event_in_catalog, create_event_catalog, create_webhook_app,
    delete_webhook_app, get_event_catalog, get_webhook_app, get_webhook_catalog,
    get_webhook_events, list_event_catalogs, list_webhook_apps, rotate_webhook_secret,
    update_event_catalog, update_webhook_app,
};
pub use deliveries::{
    get_app_webhook_deliveries, get_webhook_delivery_details, get_webhook_delivery_details_for_app,
    get_webhook_stats,
};
pub use dispatch::trigger_webhook_event;
pub use endpoints::{
    create_webhook_endpoint, create_webhook_endpoint_for_app, delete_webhook_endpoint,
    delete_webhook_endpoint_for_app, list_webhook_endpoints, reactivate_webhook_endpoint,
    test_webhook_endpoint, update_webhook_endpoint, update_webhook_endpoint_for_app,
};
pub use replay::{
    cancel_webhook_replay_task, get_webhook_replay_task_status, list_webhook_replay_tasks,
    replay_webhook_delivery,
};
