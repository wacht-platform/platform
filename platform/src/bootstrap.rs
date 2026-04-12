pub fn init_runtime_default_env(service_name: &str) {
    dotenvy::dotenv().ok();
    common::init_telemetry(service_name).expect("failed to initialize telemetry");
}

pub fn init_runtime_default_env_with_rustls(service_name: &str) {
    dotenvy::dotenv().ok();
    let _ = rustls::crypto::ring::default_provider().install_default();
    common::init_telemetry(service_name).expect("failed to initialize telemetry");
}

pub fn init_runtime_override_env_with_rustls(service_name: &str) {
    dotenvy::dotenv_override().ok();
    let _ = rustls::crypto::ring::default_provider().install_default();
    common::init_telemetry(service_name).expect("failed to initialize telemetry");
}
