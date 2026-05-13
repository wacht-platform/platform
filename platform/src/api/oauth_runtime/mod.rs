mod authorize_handlers;
pub(crate) mod helpers;
mod management_handlers;
mod oidc_handlers;
pub(crate) mod token_handlers;
pub(crate) mod types;

pub use authorize_handlers::{
    oauth_authorize_get, oauth_authorize_post, oauth_consent_submit,
    oauth_protected_resource_metadata, oauth_server_metadata,
};
pub use management_handlers::{
    oauth_delete_registered_client, oauth_get_registered_client, oauth_introspect,
    oauth_register_client, oauth_revoke, oauth_update_registered_client,
};
pub use oidc_handlers::{jwks, oauth_logout, oauth_logout_post, openid_configuration, userinfo};
pub use token_handlers::oauth_token;
