use std::collections::HashMap;
use std::sync::OnceLock;

use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{Protocol, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use tracing_subscriber::fmt;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

static LOGGER_PROVIDER: OnceLock<SdkLoggerProvider> = OnceLock::new();
static TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

fn otlp_endpoint() -> Option<String> {
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn signal_endpoint(base: &str, signal_path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), signal_path.trim_start_matches('/'))
}

fn otlp_headers() -> HashMap<String, String> {
    std::env::var("OTEL_EXPORTER_OTLP_HEADERS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|entry| {
                    let (key, value) = entry.split_once('=')?;
                    let key = key.trim();
                    let value = value.trim();
                    (!key.is_empty() && !value.is_empty())
                        .then(|| (key.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn init_telemetry(service_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = fmt::layer().with_target(true).with_thread_ids(true);
    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    if let Some(endpoint) = otlp_endpoint() {
        let resource = Resource::builder_empty()
            .with_attributes([KeyValue::new("service.name", service_name.to_string())])
            .build();
        let headers = otlp_headers();

        let trace_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(signal_endpoint(&endpoint, "/v1/traces"))
            .with_headers(headers.clone())
            .with_protocol(Protocol::HttpBinary)
            .build()?;

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(trace_exporter)
            .with_resource(resource.clone())
            .build();

        let log_exporter = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_endpoint(signal_endpoint(&endpoint, "/v1/logs"))
            .with_headers(headers)
            .with_protocol(Protocol::HttpBinary)
            .build()?;

        let logger_provider = SdkLoggerProvider::builder()
            .with_batch_exporter(log_exporter)
            .with_resource(resource)
            .build();

        let tracer = provider.tracer(service_name.to_string());
        let otel_log_layer = OpenTelemetryTracingBridge::new(&logger_provider);

        global::set_tracer_provider(provider.clone());
        let _ = LOGGER_PROVIDER.set(logger_provider);
        let _ = TRACER_PROVIDER.set(provider);

        registry
            .with(otel_log_layer)
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .try_init()?;
    } else {
        registry.try_init()?;
    }

    Ok(())
}
