use super::*;

pub(super) fn backend_domain(backend_host: &str) -> &str {
    if backend_host.starts_with("frontend.") {
        backend_host
            .strip_prefix("frontend.")
            .unwrap_or(backend_host)
    } else {
        backend_host
    }
}

pub(super) fn parse_or_generate_domain_records<D>(
    value: Option<serde_json::Value>,
    frontend_host: &str,
    backend_host: &str,
    deps: &D,
) -> Result<DomainVerificationRecords, AppError>
where
    D: common::HasCloudflareProvider + ?Sized,
{
    let records = value
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| {
            AppError::Internal(format!("Invalid domain_verification_records JSON: {}", e))
        })?
        .unwrap_or_else(|| {
            deps.cloudflare_provider()
                .generate_domain_verification_records(frontend_host, backend_host)
        });
    Ok(records)
}

pub(super) fn parse_or_default_email_records(
    value: Option<serde_json::Value>,
) -> Result<EmailVerificationRecords, AppError> {
    value
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| AppError::Internal(format!("Invalid email_verification_records JSON: {}", e)))
        .map(Option::unwrap_or_default)
}
