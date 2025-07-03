use serde::Deserialize;

#[derive(Deserialize)]
pub enum DeploymentNameParams {
    #[serde(rename = "organization-invite-template")]
    OrganizationInviteTemplate,
    #[serde(rename = "verification-code-template")]
    VerificationCodeTemplate,
    #[serde(rename = "reset-password-code-template")]
    ResetPasswordCodeTemplate,
    #[serde(rename = "primary-email-change-template")]
    PrimaryEmailChangeTemplate,
    #[serde(rename = "password-change-template")]
    PasswordChangeTemplate,
    #[serde(rename = "password-remove-template")]
    PasswordRemoveTemplate,
    #[serde(rename = "sign-in-from-new-device-template")]
    SignInFromNewDeviceTemplate,
    #[serde(rename = "magic-link-template")]
    MagicLinkTemplate,
    #[serde(rename = "waitlist-signup-template")]
    WaitlistSignupTemplate,
    #[serde(rename = "waitlist-invite-template")]
    WaitlistInviteTemplate,
    #[serde(rename = "workspace-invite-template")]
    WorkspaceInviteTemplate,
}
