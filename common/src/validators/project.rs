use crate::error::AppError;

pub struct ProjectValidator;

impl ProjectValidator {
    pub fn new() -> Self {
        Self
    }

    pub fn validate_domain_format(&self, domain: &str) -> Result<(), AppError> {
        if domain.is_empty() || domain.len() > 253 {
            return Err(AppError::BadRequest(
                "Domain must be between 1 and 253 characters".to_string(),
            ));
        }

        if domain.contains("://")
            || domain.contains("/")
            || domain.contains("?")
            || domain.contains("#")
        {
            return Err(AppError::BadRequest(
                "Domain cannot contain protocol, path, query, or fragment".to_string(),
            ));
        }

        let labels: Vec<&str> = domain.split('.').collect();
        if labels.len() < 2 {
            return Err(AppError::BadRequest(
                "Domain must have at least two labels (e.g., example.com)".to_string(),
            ));
        }

        for label in labels {
            if label.is_empty() || label.len() > 63 {
                return Err(AppError::BadRequest(
                    "Each domain label must be between 1 and 63 characters".to_string(),
                ));
            }

            if !label.chars().next().unwrap_or(' ').is_alphanumeric()
                || !label.chars().last().unwrap_or(' ').is_alphanumeric()
            {
                return Err(AppError::BadRequest(
                    "Domain labels must start and end with alphanumeric characters".to_string(),
                ));
            }

            if !label.chars().all(|c| c.is_alphanumeric() || c == '-') {
                return Err(AppError::BadRequest(
                    "Domain labels can only contain alphanumeric characters and hyphens"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }

    pub fn validate_auth_methods(&self, auth_methods: &[String]) -> Result<(), AppError> {
        if auth_methods.is_empty() {
            return Err(AppError::BadRequest(
                "At least one authentication method must be specified".to_string(),
            ));
        }

        let valid_methods = [
            "email",
            "phone",
            "username",
            "google_oauth",
            "apple_oauth",
            "facebook_oauth",
            "github_oauth",
            "microsoft_oauth",
            "discord_oauth",
            "linkedin_oauth",
            "gitlab_oauth",
            "x_oauth",
        ];

        for method in auth_methods {
            if !valid_methods.contains(&method.as_str()) {
                return Err(AppError::BadRequest(format!(
                    "Invalid authentication method: {}",
                    method
                )));
            }
        }

        Ok(())
    }

    pub fn validate_project_name(&self, name: &str) -> Result<(), AppError> {
        if name.is_empty() {
            return Err(AppError::BadRequest(
                "Project name cannot be empty".to_string(),
            ));
        }

        if name.len() > 100 {
            return Err(AppError::BadRequest(
                "Project name cannot exceed 100 characters".to_string(),
            ));
        }

        Ok(())
    }
}
