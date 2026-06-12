use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The 23 `--wa-*` SDK design tokens (plus the two optional font tokens), per
/// theme. Field names are snake_case and map to the kebab-case CSS custom
/// property suffix on the SDK side (e.g. `surface_subtle` -> `--wa-surface-subtle`).
/// Every token is optional; anything left unset falls back to the SDK default.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct WaThemeTokens {
    pub surface: Option<String>,
    pub surface_subtle: Option<String>,
    pub background: Option<String>,
    pub canvas: Option<String>,
    pub text: Option<String>,
    pub text_secondary: Option<String>,
    pub text_muted: Option<String>,
    pub text_faint: Option<String>,
    pub border: Option<String>,
    pub border_strong: Option<String>,
    pub primary: Option<String>,
    pub primary_soft: Option<String>,
    pub primary_foreground: Option<String>,
    pub success: Option<String>,
    pub success_soft: Option<String>,
    pub info: Option<String>,
    pub info_soft: Option<String>,
    pub warning: Option<String>,
    pub warning_soft: Option<String>,
    pub error: Option<String>,
    pub error_soft: Option<String>,
    pub radius: Option<String>,
    pub radius_lg: Option<String>,
    pub font_sans: Option<String>,
    pub font_mono: Option<String>,
}

/// Per-deployment override of the SDK `--wa-*` token contract, split by mode.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ThemeTokens {
    #[serde(default)]
    pub light: Option<WaThemeTokens>,
    #[serde(default)]
    pub dark: Option<WaThemeTokens>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentUISettings {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_name: String,
    pub tos_page_url: String,
    pub sign_in_page_url: String,
    pub sign_up_page_url: String,
    pub after_sign_out_one_page_url: String,
    pub after_sign_out_all_page_url: String,
    pub favicon_image_url: String,
    pub logo_image_url: String,
    pub privacy_policy_url: String,
    pub signup_terms_statement: String,
    pub signup_terms_statement_shown: bool,
    #[serde(default)]
    pub theme_tokens: Option<ThemeTokens>,
    pub after_logo_click_url: String,
    pub organization_profile_url: String,
    pub create_organization_url: String,
    pub default_user_profile_image_url: String,
    pub default_organization_profile_image_url: String,
    pub default_workspace_profile_image_url: String,
    pub use_initials_for_user_profile_image: bool,
    pub use_initials_for_organization_profile_image: bool,
    pub after_signup_redirect_url: String,
    pub after_signin_redirect_url: String,
    pub user_profile_url: String,
    pub after_create_organization_redirect_url: String,
    pub waitlist_page_url: String,
    pub support_page_url: String,
}

impl Default for DeploymentUISettings {
    fn default() -> Self {
        Self {
            id: 0,
            created_at: None,
            updated_at: None,
            deployment_id: 0,
            app_name: "".to_string(),
            tos_page_url: "".to_string(),
            sign_in_page_url: "".to_string(),
            sign_up_page_url: "".to_string(),
            after_sign_out_one_page_url: "".to_string(),
            after_sign_out_all_page_url: "".to_string(),
            favicon_image_url: "".to_string(),
            logo_image_url: "".to_string(),
            privacy_policy_url: "".to_string(),
            signup_terms_statement: "I agree to the Terms of Service and Privacy Policy"
                .to_string(),
            signup_terms_statement_shown: true,
            theme_tokens: None,
            after_logo_click_url: "".to_string(),
            organization_profile_url: "".to_string(),
            create_organization_url: "".to_string(),
            default_user_profile_image_url: "".to_string(),
            default_organization_profile_image_url: "".to_string(),
            default_workspace_profile_image_url: "".to_string(),
            use_initials_for_user_profile_image: true,
            use_initials_for_organization_profile_image: true,
            after_signup_redirect_url: "".to_string(),
            after_signin_redirect_url: "".to_string(),
            user_profile_url: "".to_string(),
            after_create_organization_redirect_url: "".to_string(),
            waitlist_page_url: "".to_string(),
            support_page_url: "".to_string(),
        }
    }
}
