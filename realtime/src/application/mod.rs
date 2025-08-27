mod error;
pub mod response;
mod router;

use common::state::AppState;

pub fn new(app_state: AppState) -> axum::Router {
    router::create_router(app_state)
}
