use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LightModeSettings {
    pub primary_color: Option<String>,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
}

impl Default for LightModeSettings {
    fn default() -> Self {
        Self {
            primary_color: Some("#6366F1".to_string()),
            background_color: Some("#FFFFFF".to_string()),
            text_color: Some("#000000".to_string()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DarkModeSettings {
    pub primary_color: Option<String>,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
}

impl Default for DarkModeSettings {
    fn default() -> Self {
        Self {
            primary_color: Some("#2A2A2A".to_string()),
            background_color: Some("#8B94FF".to_string()),
            text_color: Some("#FFFFFF".to_string()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentUISettings {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
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
    pub light_mode_settings: LightModeSettings,
    pub dark_mode_settings: DarkModeSettings,
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
            light_mode_settings: LightModeSettings::default(),
            dark_mode_settings: DarkModeSettings::default(),
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
