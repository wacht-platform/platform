mod app_handlers;
mod client_handlers;
mod grant_handlers;
mod scope_handlers;
mod types;

pub use app_handlers::{create_oauth_app, list_oauth_apps};
pub(crate) use app_handlers::{update_oauth_app, verify_oauth_app_domain};
pub(crate) use client_handlers::{
    create_oauth_client, deactivate_oauth_client, list_oauth_clients, rotate_oauth_client_secret,
    update_oauth_client,
};
pub(crate) use grant_handlers::{list_oauth_grants, revoke_oauth_grant};
pub(crate) use scope_handlers::{
    archive_oauth_scope, set_oauth_scope_mapping, unarchive_oauth_scope, update_oauth_scope,
};
