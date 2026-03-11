use super::*;
pub(in crate::project) fn next_id_from<D>(deps: &D) -> Result<i64, AppError>
where
    D: HasIdProvider + ?Sized,
{
    Ok(deps.id_provider().next_id()? as i64)
}

pub(in crate::project) const DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG: &str = "default";
pub(in crate::project) const MAX_PROJECTS_PER_BILLING_ACCOUNT: i64 = 10;
pub(in crate::project) const MAX_STAGING_DEPLOYMENTS_PER_PROJECT: i64 = 3;
pub(in crate::project) const SOCIAL_AUTH_METHODS: &[&str] = &[
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

pub(in crate::project) fn is_social_auth_method(method: &str) -> bool {
    SOCIAL_AUTH_METHODS.contains(&method)
}

pub(in crate::project) fn includes_phone_auth(auth_methods: &[String]) -> bool {
    auth_methods.iter().any(|method| method == "phone")
}

pub(in crate::project) fn ensure_billing_status_active(
    status: &str,
    target: &str,
) -> Result<(), AppError> {
    if status != "active" {
        return Err(AppError::Validation(format!(
            "Cannot create {}. Billing account status is {}",
            target, status
        )));
    }

    Ok(())
}

pub(in crate::project) fn ensure_phone_auth_allowed(
    auth_methods: &[String],
    pulse_usage_disabled: bool,
) -> Result<(), AppError> {
    if includes_phone_auth(auth_methods) && pulse_usage_disabled {
        return Err(AppError::Validation(
            "Prepaid recharge is required before enabling phone authentication for staging deployments".to_string(),
        ));
    }

    Ok(())
}

pub(in crate::project) fn project_not_found(project_id: i64) -> AppError {
    AppError::NotFound(format!("Project with id {} not found", project_id))
}

pub(in crate::project) fn positive_or_default(value: i64, default: i64) -> i64 {
    if value > 0 { value } else { default }
}

pub(in crate::project) fn social_credentials_with_default_scopes(
    provider: &SocialConnectionProvider,
) -> Result<serde_json::Value, AppError> {
    serde_json::to_value(OauthCredentials {
        scopes: provider.default_scopes(),
        ..OauthCredentials::default()
    })
    .map_err(|e| AppError::Serialization(e.to_string()))
}

pub(in crate::project) fn generate_signing_secret() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    format!("whsec_{}", STANDARD.encode(bytes))
}

pub(in crate::project) fn generate_nanoid() -> String {
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

pub(in crate::project) async fn generate_deployment_key_pairs()
-> Result<(String, String, String, String), AppError> {
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

pub(in crate::project) async fn generate_deployment_key_material()
-> Result<DeploymentKeyMaterial, AppError> {
    Ok(DeploymentKeyMaterial::from_tuple(
        generate_deployment_key_pairs().await?,
    ))
}

pub(in crate::project) fn build_publishable_key(prefix: &str, backend_host: &str) -> String {
    let mut publishable_key = prefix.to_string();
    let base64_backend_host = BASE64_STANDARD.encode(format!("https://{}", backend_host));
    publishable_key.push_str(&base64_backend_host);
    publishable_key
}

pub(in crate::project) struct StagingDeploymentHosts {
    pub(in crate::project) backend_host: String,
    pub(in crate::project) frontend_host: String,
    pub(in crate::project) publishable_key: String,
}

pub(in crate::project) fn build_staging_deployment_hosts() -> StagingDeploymentHosts {
    let hostname = generate_nanoid();
    let backend_host = format!("{}.fapi.trywacht.xyz", hostname);
    let frontend_host = format!("{}.accounts.trywacht.xyz", hostname);
    let publishable_key = build_publishable_key("pk_test_", &backend_host);

    StagingDeploymentHosts {
        backend_host,
        frontend_host,
        publishable_key,
    }
}

pub(in crate::project) struct ProductionDeploymentHosts {
    pub(in crate::project) backend_host: String,
    pub(in crate::project) frontend_host: String,
    pub(in crate::project) mail_from_host: String,
    pub(in crate::project) publishable_key: String,
}

pub(in crate::project) fn build_production_deployment_hosts(
    custom_domain: &str,
) -> ProductionDeploymentHosts {
    let backend_host = format!("frontend.{}", custom_domain);
    let frontend_host = format!("accounts.{}", custom_domain);
    let mail_from_host = format!("wcmail.{}", custom_domain);
    let publishable_key = build_publishable_key("pk_live_", &backend_host);

    ProductionDeploymentHosts {
        backend_host,
        frontend_host,
        mail_from_host,
        publishable_key,
    }
}

pub(in crate::project) fn build_b2b_settings(deployment_id: i64) -> DeploymentB2bSettingsWithRoles {
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

pub(in crate::project) fn build_auth_settings(
    auth_methods: &[String],
    deployment_id: i64,
) -> DeploymentAuthSettings {
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

pub(in crate::project) fn build_ui_settings(
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

pub(in crate::project) fn build_restrictions(deployment_id: i64) -> DeploymentRestrictions {
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

pub(in crate::project) fn build_sms_templates(deployment_id: i64) -> DeploymentSmsTemplate {
    DeploymentSmsTemplate {
        deployment_id,
        ..Default::default()
    }
}

pub(in crate::project) fn build_email_templates(deployment_id: i64) -> DeploymentEmailTemplate {
    DeploymentEmailTemplate {
        deployment_id,
        ..Default::default()
    }
}

pub(in crate::project) struct DeploymentKeyMaterial {
    pub(in crate::project) public_key: String,
    pub(in crate::project) private_key: String,
    pub(in crate::project) saml_public_key: String,
    pub(in crate::project) saml_private_key: String,
}

impl DeploymentKeyMaterial {
    fn from_tuple(tuple: (String, String, String, String)) -> Self {
        let (public_key, private_key, saml_public_key, saml_private_key) = tuple;
        Self {
            public_key,
            private_key,
            saml_public_key,
            saml_private_key,
        }
    }
}

pub(in crate::project) struct DeploymentBootstrapInput<'a> {
    pub(in crate::project) deployment_id: i64,
    pub(in crate::project) frontend_host: &'a str,
    pub(in crate::project) app_name: String,
    pub(in crate::project) auth_methods: &'a [String],
    pub(in crate::project) waitlist_page_url: String,
    pub(in crate::project) support_page_url: &'a str,
    pub(in crate::project) key_material: DeploymentKeyMaterial,
}
