use common::error::AppError;
use common::validators::ProjectValidator;
use common::{CloudflareService, DnsVerificationService, HasIdGenerator, PostmarkService};
use models::{
    AuthFactorsEnabled, CustomSmtpConfig, DarkModeSettings, Deployment, DeploymentAuthSettings,
    DeploymentB2bSettings, DeploymentB2bSettingsWithRoles, DeploymentEmailTemplate, DeploymentMode,
    DeploymentOrganizationRole, DeploymentRestrictions, DeploymentSmsTemplate,
    DeploymentUISettings, DeploymentWorkspaceRole, EmailProvider, EmailSettings,
    EmailVerificationRecords, FirstFactor, IndividualAuthSettings, LightModeSettings,
    OauthCredentials, PasswordSettings, PhoneSettings, ProjectWithDeployments, SecondFactorPolicy,
    SocialConnectionProvider, UsernameSettings, VerificationPolicy, VerificationStatus,
};

use base64::{Engine, engine::general_purpose::STANDARD, prelude::BASE64_STANDARD};
use std::env;
use std::str::FromStr;

pub trait IdGenerator: Send + Sync {
    fn next_id(&self) -> Result<i64, AppError>;
}

pub struct DepsIdGeneratorAdapter<'a, D>
where
    D: HasIdGenerator + Sync,
{
    deps: &'a D,
}

impl<'a, D> DepsIdGeneratorAdapter<'a, D>
where
    D: HasIdGenerator + Sync,
{
    pub fn new(deps: &'a D) -> Self {
        Self { deps }
    }
}

impl<D> IdGenerator for DepsIdGeneratorAdapter<'_, D>
where
    D: HasIdGenerator + Sync,
{
    fn next_id(&self) -> Result<i64, AppError> {
        Ok(self.deps.id_generator().next_id()? as i64)
    }
}

pub struct ProductionDeploymentDeps<'a> {
    pub ids: &'a dyn IdGenerator,
    pub cloudflare_service: &'a CloudflareService,
    pub postmark_service: &'a PostmarkService,
}

pub struct VerifyDeploymentDnsDeps<'a> {
    pub db_router: &'a common::DbRouter,
    pub cloudflare_service: &'a CloudflareService,
    pub dns_verification_service: &'a DnsVerificationService,
}

const DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG: &str = "default";
const MAX_PROJECTS_PER_BILLING_ACCOUNT: i64 = 5;
const SOCIAL_AUTH_METHODS: &[&str] = &[
    "google",
    "apple",
    "facebook",
    "github",
    "microsoft",
    "discord",
    "linkedin",
    "x",
    "gitlab",
    "google_oauth",
    "apple_oauth",
    "facebook_oauth",
    "github_oauth",
    "microsoft_oauth",
    "discord_oauth",
    "linkedin_oauth",
    "x_oauth",
    "gitlab_oauth",
];

fn is_social_auth_method(method: &str) -> bool {
    SOCIAL_AUTH_METHODS.contains(&method)
}

fn includes_phone_auth(auth_methods: &[String]) -> bool {
    auth_methods.iter().any(|method| method == "phone")
}

fn social_credentials_with_default_scopes(
    provider: &SocialConnectionProvider,
) -> Result<serde_json::Value, AppError> {
    serde_json::to_value(OauthCredentials {
        scopes: provider.default_scopes(),
        ..OauthCredentials::default()
    })
    .map_err(|e| AppError::Serialization(e.to_string()))
}

fn generate_signing_secret() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    format!("whsec_{}", STANDARD.encode(bytes))
}

fn generate_nanoid() -> String {
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

async fn generate_deployment_key_pairs() -> Result<(String, String, String, String), AppError> {
    let key_pair_task = tokio::task::spawn_blocking(|| {
        let ecdsa_pair =
            rcgen::KeyPair::generate().map_err(|e| AppError::Internal(e.to_string()))?;

        let saml_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256).map_err(|e| {
            AppError::Internal(format!("Failed to generate SAML RSA keypair: {}", e))
        })?;

        Ok::<_, AppError>((
            ecdsa_pair.public_key_pem(),
            ecdsa_pair.serialize_pem(),
            saml_pair.public_key_pem(),
            saml_pair.serialize_pem(),
        ))
    });

    key_pair_task
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn build_b2b_settings(deployment_id: i64) -> DeploymentB2bSettingsWithRoles {
    DeploymentB2bSettingsWithRoles {
        settings: DeploymentB2bSettings {
            deployment_id,
            ..DeploymentB2bSettings::default()
        },
        default_workspace_creator_role: DeploymentWorkspaceRole::admin(),
        default_workspace_member_role: DeploymentWorkspaceRole::member(),
        default_org_creator_role: DeploymentOrganizationRole::admin(),
        default_org_member_role: DeploymentOrganizationRole::member(),
    }
}

fn build_auth_settings(auth_methods: &[String], deployment_id: i64) -> DeploymentAuthSettings {
    let email_enabled = auth_methods.iter().any(|method| method == "email");
    let phone_enabled = auth_methods.iter().any(|method| method == "phone");
    let username_enabled = auth_methods.iter().any(|method| method == "username");

    let mut first_factor = FirstFactor::EmailPassword;
    let mut alternate_first_factors: Vec<FirstFactor> = Vec::new();

    if email_enabled {
        first_factor = FirstFactor::EmailPassword;
        if phone_enabled {
            alternate_first_factors.push(FirstFactor::PhoneOtp);
        }
        if username_enabled {
            alternate_first_factors.push(FirstFactor::UsernamePassword);
        }
    } else if phone_enabled {
        first_factor = FirstFactor::PhoneOtp;
        if username_enabled {
            alternate_first_factors.push(FirstFactor::UsernamePassword);
        }
    } else if username_enabled {
        first_factor = FirstFactor::UsernamePassword;
    }

    let email_settings = EmailSettings {
        enabled: email_enabled,
        required: email_enabled,
        ..EmailSettings::default()
    };

    let phone_settings = PhoneSettings {
        enabled: phone_enabled,
        required: phone_enabled,
        ..PhoneSettings::default()
    };

    let username_settings = UsernameSettings {
        enabled: username_enabled,
        required: username_enabled,
        ..UsernameSettings::default()
    };

    let password_settings = PasswordSettings::default();
    let first_name_settings = IndividualAuthSettings::default();
    let last_name_settings = IndividualAuthSettings::default();

    let auth_factors_enabled = AuthFactorsEnabled::default()
        .with_email(email_enabled)
        .with_phone(phone_enabled)
        .with_username(username_enabled);

    let verification_policy = VerificationPolicy {
        phone_number: phone_enabled,
        email: email_enabled,
    };

    DeploymentAuthSettings {
        deployment_id,
        email_address: email_settings,
        phone_number: phone_settings,
        username: username_settings,
        first_factor,
        first_name: first_name_settings,
        last_name: last_name_settings,
        password: password_settings,
        auth_factors_enabled,
        verification_policy,
        second_factor_policy: SecondFactorPolicy::None,
        ..DeploymentAuthSettings::default()
    }
}

fn build_ui_settings(
    deployment_id: i64,
    frontend_host: &str,
    app_name: String,
) -> DeploymentUISettings {
    let frontend_url = format!("https://{}", frontend_host);

    DeploymentUISettings {
        deployment_id,
        app_name,
        after_sign_out_all_page_url: format!("{}/sign-in", frontend_url),
        after_sign_out_one_page_url: frontend_url.clone(),
        sign_in_page_url: format!("{}/sign-in", frontend_url),
        sign_up_page_url: format!("{}/sign-up", frontend_url),
        dark_mode_settings: DarkModeSettings::default(),
        light_mode_settings: LightModeSettings::default(),
        organization_profile_url: format!("{}/organization", frontend_url),
        create_organization_url: format!("{}/create-organization", frontend_url),
        user_profile_url: format!("{}/me", frontend_url),
        use_initials_for_organization_profile_image: true,
        use_initials_for_user_profile_image: true,
        ..DeploymentUISettings::default()
    }
}

fn build_restrictions(deployment_id: i64) -> DeploymentRestrictions {
    DeploymentRestrictions {
        deployment_id,
        allowlist_enabled: false,
        blocklist_enabled: false,
        block_subaddresses: false,
        block_disposable_emails: false,
        block_voip_numbers: false,
        country_restrictions: Default::default(),
        banned_keywords: Default::default(),
        allowlisted_resources: Default::default(),
        blocklisted_resources: Default::default(),
        sign_up_mode: Default::default(),
        waitlist_collect_names: true,
        ..Default::default()
    }
}

fn build_sms_templates(deployment_id: i64) -> DeploymentSmsTemplate {
    DeploymentSmsTemplate {
        deployment_id,
        ..Default::default()
    }
}

fn build_email_templates(deployment_id: i64) -> DeploymentEmailTemplate {
    DeploymentEmailTemplate {
        deployment_id,
        ..Default::default()
    }
}

fn json_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, AppError> {
    serde_json::to_value(value).map_err(|e| AppError::Serialization(e.to_string()))
}

fn console_deployment_id() -> Result<i64, AppError> {
    let raw = env::var("CONSOLE_DEPLOYMENT_ID").map_err(|_| {
        AppError::Internal("CONSOLE_DEPLOYMENT_ID environment variable is not set".to_string())
    })?;

    raw.parse::<i64>().map_err(|e| {
        AppError::Internal(format!(
            "CONSOLE_DEPLOYMENT_ID must be a valid i64, got '{}': {}",
            raw, e
        ))
    })
}

mod blocks;
mod create_production_deployment;
mod create_project_with_staging;
mod create_staging_deployment;
mod delete_project;
mod verify_deployment_dns_records;

use blocks::*;

pub use create_production_deployment::CreateProductionDeploymentCommand;
pub use create_project_with_staging::CreateProjectWithStagingDeploymentCommand;
pub use create_staging_deployment::CreateStagingDeploymentCommand;
pub use delete_project::DeleteProjectCommand;
pub use verify_deployment_dns_records::VerifyDeploymentDnsRecordsCommand;
