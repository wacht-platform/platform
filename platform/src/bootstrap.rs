use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

pub fn init_runtime_default_env() {
    dotenvy::dotenv().ok();
    init_tracing();
}

pub fn init_runtime_default_env_with_rustls() {
    dotenvy::dotenv().ok();
    let _ = rustls::crypto::ring::default_provider().install_default();
    init_tracing();
}

pub fn init_runtime_override_env_with_rustls() {
    dotenvy::dotenv_override().ok();
    let _ = rustls::crypto::ring::default_provider().install_default();
    init_tracing();
}
