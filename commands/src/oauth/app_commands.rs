use common::json_utils::json_default;
use common::{HasCloudflareProvider, HasDbRouter, error::AppError};
use models::api_key::OAuthScopeDefinition;
use queries::oauth::OAuthAppData;

fn normalize_supported_scopes(scopes: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = scopes
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    out.sort();
    out.dedup();
    out
}

fn normalize_scope_definitions(
    supported_scopes: &[String],
    input: Option<Vec<OAuthScopeDefinition>>,
) -> Result<Vec<OAuthScopeDefinition>, AppError> {
    let mut by_scope = std::collections::BTreeMap::<String, OAuthScopeDefinition>::new();

    if let Some(defs) = input {
        for mut def in defs {
            let scope = def.scope.trim().to_string();
            if scope.is_empty() {
                continue;
            }
            def.scope = scope.clone();
            by_scope.insert(scope, def);
        }
    }

    supported_scopes
        .iter()
        .map(|scope| {
            if let Some(mut def) = by_scope.remove(scope) {
                if def.display_name.trim().is_empty() {
                    def.display_name = scope.to_string();
                }
                if def.description.trim().is_empty() {
                    def.description = format!("Allows {} access", scope);
                }
                def.category = parse_scope_category(def.category)?;
                def.organization_permission = def
                    .organization_permission
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToOwned::to_owned);
                def.workspace_permission = def
                    .workspace_permission
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToOwned::to_owned);
                validate_scope_mapping(scope, &def)?;
                return Ok(def);
            }

            let def = OAuthScopeDefinition {
                scope: scope.to_string(),
                display_name: scope.to_string(),
                description: format!("Allows {} access", scope),
                archived: false,
                category: String::new(),
                organization_permission: None,
                workspace_permission: None,
            };
            validate_scope_mapping(scope, &def)?;
            Ok(def)
        })
        .collect::<Result<Vec<_>, AppError>>()
}

fn parse_scope_category(raw: String) -> Result<String, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => Ok(String::new()),
        "personal" => Ok("personal".to_string()),
        "organization" => Ok("organization".to_string()),
        "workspace" => Ok("workspace".to_string()),
        _ => Err(AppError::Validation(
            "scope category must be one of: personal, organization, workspace".to_string(),
        )),
    }
}

fn validate_scope_mapping(scope: &str, def: &OAuthScopeDefinition) -> Result<(), AppError> {
    match def.category.as_str() {
        "" => {
            if def.organization_permission.is_some() || def.workspace_permission.is_some() {
                return Err(AppError::Validation(format!(
                    "scope '{}' without category cannot define permission mappings",
                    scope
                )));
            }
        }
        "personal" => {
            if def.organization_permission.is_some() || def.workspace_permission.is_some() {
                return Err(AppError::Validation(format!(
                    "scope '{}' with category 'personal' cannot define organization/workspace permissions",
                    scope
                )));
            }
        }
        "organization" => {
            if def.workspace_permission.is_some() {
                return Err(AppError::Validation(format!(
                    "scope '{}' with category 'organization' cannot define workspace permission",
                    scope
                )));
            }
        }
        "workspace" => {
            if def.organization_permission.is_some() {
                return Err(AppError::Validation(format!(
                    "scope '{}' with category 'workspace' cannot define organization permission",
                    scope
                )));
            }
        }
        _ => {
            return Err(AppError::Validation(format!(
                "scope '{}' has invalid category '{}'",
                scope, def.category
            )));
        }
    }
    Ok(())
}

fn build_oauth_fqdn(mode: &str, fqdn: Option<&str>) -> Result<String, AppError> {
    if mode.eq_ignore_ascii_case("production") {
        let value = fqdn
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| AppError::Validation("fqdn is required for production".to_string()))?;
        validate_fqdn(value)?;
        return Ok(value.to_ascii_lowercase());
    }

    let label = generate_oauth_domain_label();
    validate_dns_label(&label, "fqdn")?;
    Ok(format!("{}.oapi.trywacht.xyz", label))
}

fn validate_dns_label(value: &str, field_name: &str) -> Result<(), AppError> {
    if value.is_empty() || value.len() > 63 {
        return Err(AppError::Validation(format!(
            "{} must be between 1 and 63 characters",
            field_name
        )));
    }

    if value.starts_with('-') || value.ends_with('-') {
        return Err(AppError::Validation(format!(
            "{} cannot start or end with '-'",
            field_name
        )));
    }

    if !value.bytes().all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-'
    }) {
        return Err(AppError::Validation(format!(
            "{} must contain only letters, numbers, or '-'",
            field_name
        )));
    }

    Ok(())
}

fn generate_oauth_domain_label() -> String {
    const EDGE_ALPHABET: [char; 36] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    ];
    const MIDDLE_ALPHABET: [char; 37] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        '-',
    ];
    const LABEL_LEN: usize = 16;
    const MIDDLE_LEN: usize = LABEL_LEN - 2;
    let first = nanoid::nanoid!(1, &EDGE_ALPHABET);
    let middle = nanoid::nanoid!(MIDDLE_LEN, &MIDDLE_ALPHABET);
    let last = nanoid::nanoid!(1, &EDGE_ALPHABET);
    format!("{}{}{}", first, middle, last)
}

fn validate_fqdn(value: &str) -> Result<(), AppError> {
    if value.is_empty() || value.len() > 253 {
        return Err(AppError::Validation(
            "fqdn must be between 1 and 253 characters".to_string(),
        ));
    }
    if value.starts_with('.') || value.ends_with('.') {
        return Err(AppError::Validation(
            "fqdn cannot start or end with '.'".to_string(),
        ));
    }
    if !value.contains('.') {
        return Err(AppError::Validation(
            "fqdn must be a full domain name".to_string(),
        ));
    }

    for label in value.split('.') {
        validate_dns_label(label, "fqdn")?;
    }

    Ok(())
}

mod create_app;
mod update_app;
mod verify_domain;

pub use create_app::*;
pub use update_app::*;
pub use verify_domain::*;
