mod router;

pub use shared::state::AppState;

pub fn new(app_state: AppState) -> axum::Router {
    router::create_router(app_state)
}
