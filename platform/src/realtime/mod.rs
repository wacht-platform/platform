pub mod api;
pub mod application;
pub mod middleware;

use common::state::AppState;

pub fn router(app_state: AppState) -> axum::Router {
    application::new(app_state)
}
