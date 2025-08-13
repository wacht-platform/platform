mod error;
pub mod response;
mod router;

pub use common::error::AppError;
pub use common::state::AppState as HttpState;

pub async fn new(app_state: HttpState) -> axum::Router {
    router::create_router(app_state).await
}
