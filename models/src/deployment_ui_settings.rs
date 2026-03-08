use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct TokenOverrides {
    pub space_unit: Option<String>,
    pub foreground: Option<String>,
    pub foreground_inverse: Option<String>,
    pub secondary_text: Option<String>,
    pub muted: Option<String>,
    pub border: Option<String>,
    pub border_hover: Option<String>,
    pub divider: Option<String>,
    pub input_background: Option<String>,
    pub input_border: Option<String>,
    pub input_focus_border: Option<String>,
    pub background_subtle: Option<String>,
    pub background_hover: Option<String>,
    pub primary_hover: Option<String>,
    pub error: Option<String>,
    pub error_background: Option<String>,
    pub error_border: Option<String>,
    pub warning: Option<String>,
    pub warning_background: Option<String>,
    pub warning_border: Option<String>,
    pub warning_text: Option<String>,
    pub success: Option<String>,
    pub success_background: Option<String>,
    pub success_border: Option<String>,
    pub info: Option<String>,
    pub info_background: Option<String>,
    pub radius_md: Option<String>,
    pub radius_lg: Option<String>,
    pub radius_xl: Option<String>,
    pub radius_2xl: Option<String>,
    pub radius_2xs: Option<String>,
    pub radius_xs: Option<String>,
    pub radius_full: Option<String>,
    pub border_width_thin: Option<String>,
    pub border_width_regular: Option<String>,
    pub scrollbar_track: Option<String>,
    pub scrollbar_thumb: Option<String>,
    pub scrollbar_thumb_hover: Option<String>,
    pub shadow_color: Option<String>,
    pub shadow_light_color: Option<String>,
    pub shadow_medium_color: Option<String>,
    pub success_shadow: Option<String>,
    pub success_background_light: Option<String>,
    pub button_ripple: Option<String>,
    pub dialog_backdrop: Option<String>,
    pub space_0u: Option<String>,
    pub space_1u: Option<String>,
    pub space_2u: Option<String>,
    pub space_3u: Option<String>,
    pub space_4u: Option<String>,
    pub space_5u: Option<String>,
    pub space_6u: Option<String>,
    pub space_7u: Option<String>,
    pub space_8u: Option<String>,
    pub space_10u: Option<String>,
    pub space_12u: Option<String>,
    pub space_14u: Option<String>,
    pub space_16u: Option<String>,
    pub space_24u: Option<String>,
    pub font_size_2xs: Option<String>,
    pub font_size_xs: Option<String>,
    pub font_size_sm: Option<String>,
    pub font_size_md: Option<String>,
    pub font_size_lg: Option<String>,
    pub font_size_xl: Option<String>,
    pub font_size_2xl: Option<String>,
    pub font_size_3xl: Option<String>,
    pub size_8u: Option<String>,
    pub size_10u: Option<String>,
    pub size_12u: Option<String>,
    pub size_18u: Option<String>,
    pub size_20u: Option<String>,
    pub size_24u: Option<String>,
    pub size_32u: Option<String>,
    pub size_36u: Option<String>,
    pub size_40u: Option<String>,
    pub size_45u: Option<String>,
    pub size_50u: Option<String>,
    pub shadow_sm: Option<String>,
    pub shadow_md: Option<String>,
    pub shadow_lg: Option<String>,
    pub shadow_xl: Option<String>,
    pub shadow_success: Option<String>,
    pub ring_primary: Option<String>,
    pub letter_spacing_tight: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LightModeSettings {
    pub primary_color: Option<String>,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
    #[serde(default)]
    pub token_overrides: Option<TokenOverrides>,
}

impl Default for LightModeSettings {
    fn default() -> Self {
        Self {
            primary_color: Some("#6366F1".to_string()),
            background_color: Some("#FFFFFF".to_string()),
            text_color: Some("#000000".to_string()),
            token_overrides: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DarkModeSettings {
    pub primary_color: Option<String>,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
    #[serde(default)]
    pub token_overrides: Option<TokenOverrides>,
}

impl Default for DarkModeSettings {
    fn default() -> Self {
        Self {
            primary_color: Some("#2A2A2A".to_string()),
            background_color: Some("#8B94FF".to_string()),
            text_color: Some("#FFFFFF".to_string()),
            token_overrides: None,
        }
    }
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
