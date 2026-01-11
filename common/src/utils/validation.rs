use models::{
    DeploymentAuthSettings, EmailSettings, PasswordSettings, PhoneSettings, UsernameSettings,
};
use regex::Regex;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl ValidationError {
    pub fn new(field: &str, message: &str) -> Self {
        Self {
            field: field.to_string(),
            message: message.to_string(),
        }
    }
}

pub struct UserValidator;

impl UserValidator {
    pub fn validate_user_creation(
        first_name: &str,
        last_name: &str,
        email: &Option<String>,
        phone: &Option<String>,
        username: &Option<String>,
        password: &Option<String>,
        skip_password_check: bool,
        auth_settings: &DeploymentAuthSettings,
    ) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        // Validate first name
        if auth_settings.first_name.required.unwrap_or(false) && first_name.trim().is_empty() {
            errors.push(ValidationError::new("first_name", "First name is required"));
        }

        // Validate last name
        if auth_settings.last_name.required.unwrap_or(false) && last_name.trim().is_empty() {
            errors.push(ValidationError::new("last_name", "Last name is required"));
        }

        // Validate email
        if let Err(email_errors) = Self::validate_email(email, &auth_settings.email_address) {
            errors.extend(email_errors);
        }

        // Validate phone
        if let Err(phone_errors) = Self::validate_phone(phone, &auth_settings.phone_number) {
            errors.extend(phone_errors);
        }

        // Validate username
        if let Err(username_errors) = Self::validate_username(username, &auth_settings.username) {
            errors.extend(username_errors);
        }

        // Validate password
        if !skip_password_check {
            if let Err(password_errors) = Self::validate_password(password, &auth_settings.password)
            {
                errors.extend(password_errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_email(
        email: &Option<String>,
        settings: &EmailSettings,
    ) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        if settings.required {
            match email {
                None => {
                    errors.push(ValidationError::new(
                        "email_address",
                        "Email address is required",
                    ));
                }
                Some(e) if e.trim().is_empty() => {
                    errors.push(ValidationError::new(
                        "email_address",
                        "Email address is required",
                    ));
                }
                Some(email_str) => {
                    if !Self::is_valid_email(email_str) {
                        errors.push(ValidationError::new(
                            "email_address",
                            "Invalid email format",
                        ));
                    }
                }
            }
        } else if let Some(email_str) = email {
            if !email_str.trim().is_empty() && !Self::is_valid_email(email_str) {
                errors.push(ValidationError::new(
                    "email_address",
                    "Invalid email format",
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_phone(
        phone: &Option<String>,
        settings: &PhoneSettings,
    ) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        if settings.required {
            match phone {
                None => {
                    errors.push(ValidationError::new(
                        "phone_number",
                        "Phone number is required",
                    ));
                }
                Some(p) if p.trim().is_empty() => {
                    errors.push(ValidationError::new(
                        "phone_number",
                        "Phone number is required",
                    ));
                }
                Some(phone_str) => {
                    if !Self::is_valid_phone(phone_str) {
                        errors.push(ValidationError::new(
                            "phone_number",
                            "Invalid phone number format",
                        ));
                    }
                }
            }
        } else if let Some(phone_str) = phone {
            if !phone_str.trim().is_empty() && !Self::is_valid_phone(phone_str) {
                errors.push(ValidationError::new(
                    "phone_number",
                    "Invalid phone number format",
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_username(
        username: &Option<String>,
        settings: &UsernameSettings,
    ) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        if settings.required {
            match username {
                None => {
                    errors.push(ValidationError::new("username", "Username is required"));
                }
                Some(u) if u.trim().is_empty() => {
                    errors.push(ValidationError::new("username", "Username is required"));
                }
                Some(username_str) => {
                    Self::validate_username_constraints(username_str, settings, &mut errors);
                }
            }
        } else if let Some(username_str) = username {
            if !username_str.trim().is_empty() {
                Self::validate_username_constraints(username_str, settings, &mut errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_username_constraints(
        username: &str,
        settings: &UsernameSettings,
        errors: &mut Vec<ValidationError>,
    ) {
        let len = username.len();

        if let Some(min_len) = settings.min_length {
            if len < min_len as usize {
                errors.push(ValidationError::new(
                    "username",
                    &format!("Username must be at least {} characters", min_len),
                ));
            }
        }

        if let Some(max_len) = settings.max_length {
            if len > max_len as usize {
                errors.push(ValidationError::new(
                    "username",
                    &format!("Username must be at most {} characters", max_len),
                ));
            }
        }

        // Username should only contain alphanumeric characters, underscores, and hyphens
        if !username
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            errors.push(ValidationError::new(
                "username",
                "Username can only contain letters, numbers, underscores, and hyphens",
            ));
        }
    }

    pub fn validate_password(
        password: &Option<String>,
        settings: &PasswordSettings,
    ) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        if !settings.enabled {
            return Ok(());
        }

        match password {
            None => {
                errors.push(ValidationError::new("password", "Password is required"));
            }
            Some(p) if p.trim().is_empty() => {
                errors.push(ValidationError::new("password", "Password is required"));
            }
            Some(password_str) => {
                Self::validate_password_constraints(password_str, settings, &mut errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_password_constraints(
        password: &str,
        settings: &PasswordSettings,
        errors: &mut Vec<ValidationError>,
    ) {
        if let Some(min_len) = settings.min_length {
            if password.len() < min_len as usize {
                errors.push(ValidationError::new(
                    "password",
                    &format!("Password must be at least {} characters", min_len),
                ));
            }
        }

        if settings.require_lowercase.unwrap_or(false)
            && !password.chars().any(|c| c.is_lowercase())
        {
            errors.push(ValidationError::new(
                "password",
                "Password must contain at least one lowercase letter",
            ));
        }

        if settings.require_uppercase.unwrap_or(false)
            && !password.chars().any(|c| c.is_uppercase())
        {
            errors.push(ValidationError::new(
                "password",
                "Password must contain at least one uppercase letter",
            ));
        }

        if settings.require_number.unwrap_or(false) && !password.chars().any(|c| c.is_numeric()) {
            errors.push(ValidationError::new(
                "password",
                "Password must contain at least one number",
            ));
        }

        if settings.require_special.unwrap_or(false)
            && !password
                .chars()
                .any(|c| "!@#$%^&*()_+-=[]{}|;:,.<>?".contains(c))
        {
            errors.push(ValidationError::new(
                "password",
                "Password must contain at least one special character",
            ));
        }
    }

    fn is_valid_email(email: &str) -> bool {
        let email_regex = Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap();
        email_regex.is_match(email)
    }

    fn is_valid_phone(phone: &str) -> bool {
        // Basic phone validation - starts with + and contains only digits, spaces, hyphens, parentheses
        let phone_regex = Regex::new(r"^\+?[1-9]\d{1,14}$|^\+?[1-9][\d\s\-\(\)]{7,20}$").unwrap();
        let cleaned = phone.replace(&[' ', '-', '(', ')'][..], "");
        phone_regex.is_match(&cleaned)
    }
}

/// Host validation utility functions
pub struct HostValidator;

impl HostValidator {
    /// Validates that a host is not an IP address (IPv4 or IPv6)
    /// Returns true if the host is valid (not an IP address)
    pub fn is_valid_host(host: &str) -> bool {
        // Try to parse as an IP address
        match host.parse::<IpAddr>() {
            Ok(_) => false, // It's an IP address, so invalid
            Err(_) => true, // Not an IP address, so valid
        }
    }

    /// Validates that a host is not an IPv4 address
    pub fn is_not_ipv4(host: &str) -> bool {
        host.parse::<Ipv4Addr>().is_err()
    }

    /// Validates that a host is not an IPv6 address
    pub fn is_not_ipv6(host: &str) -> bool {
        host.parse::<Ipv6Addr>().is_err()
    }
}
