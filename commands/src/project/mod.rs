use common::error::AppError;
use common::validators::ProjectValidator;
use common::HasIdProvider;
use models::{
    AuthFactorsEnabled, CustomSmtpConfig, DarkModeSettings, Deployment, DeploymentAuthSettings,
    DeploymentB2bSettings, DeploymentB2bSettingsWithRoles, DeploymentEmailTemplate, DeploymentMode,
    DeploymentOrganizationRole, DeploymentRestrictions, DeploymentSmsTemplate,
    DeploymentUISettings, DeploymentWorkspaceRole, EmailProvider, EmailSettings,
    EmailVerificationRecords, FirstFactor, IndividualAuthSettings, LightModeSettings,
    DomainVerificationRecords,
    OauthCredentials, PasswordSettings, PhoneSettings, ProjectWithDeployments, SecondFactorPolicy,
    SocialConnectionProvider, UsernameSettings, VerificationPolicy, VerificationStatus,
};

use base64::{Engine, engine::general_purpose::STANDARD, prelude::BASE64_STANDARD};
use std::env;
use std::str::FromStr;

mod support;
use support::*;
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
