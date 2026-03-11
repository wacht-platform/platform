use super::*;

fn next_b2b_role_ids<D>(deps: &D) -> Result<(i64, i64, i64, i64), AppError>
where
    D: HasIdProvider + ?Sized,
{
    Ok((
        next_id_from(deps)?,
        next_id_from(deps)?,
        next_id_from(deps)?,
        next_id_from(deps)?,
    ))
}

async fn insert_core_default_settings<D>(
    conn: &mut sqlx::PgConnection,
    deps: &D,
    input: &DeploymentBootstrapInput<'_>,
) -> Result<(), AppError>
where
    D: HasIdProvider + ?Sized,
{
    let auth_settings = build_auth_settings(input.auth_methods, input.deployment_id);
    DeploymentAuthSettingsInsert::builder()
        .id(next_id_from(deps)?)
        .auth_settings(auth_settings)
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    let ui_settings = build_ui_settings(
        input.deployment_id,
        input.frontend_host,
        input.app_name.clone(),
    );
    DeploymentUiSettingsInsert::builder()
        .id(next_id_from(deps)?)
        .ui_settings(ui_settings)
        .waitlist_page_url(input.waitlist_page_url.clone())
        .support_page_url(input.support_page_url)
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    let restrictions = build_restrictions(input.deployment_id);
    DeploymentRestrictionsInsert::builder()
        .id(next_id_from(deps)?)
        .restrictions(restrictions)
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    let sms_templates = build_sms_templates(input.deployment_id);
    DeploymentSmsTemplatesInsert::builder()
        .id(next_id_from(deps)?)
        .sms_templates(sms_templates)
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    let email_templates = build_email_templates(input.deployment_id);
    DeploymentEmailTemplatesInsert::builder()
        .id(next_id_from(deps)?)
        .email_templates(email_templates)
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    DeploymentKeyPairsInsert::builder()
        .id(next_id_from(deps)?)
        .deployment_id(input.deployment_id)
        .public_key(input.key_material.public_key.clone())
        .private_key(input.key_material.private_key.clone())
        .saml_public_key(input.key_material.saml_public_key.clone())
        .saml_private_key(input.key_material.saml_private_key.clone())
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    DeploymentAiSettingsInsert::builder()
        .id(next_id_from(deps)?)
        .deployment_id(input.deployment_id)
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    Ok(())
}

async fn insert_external_default_bootstraps<D>(
    conn: &mut sqlx::PgConnection,
    deps: &D,
    input: &DeploymentBootstrapInput<'_>,
) -> Result<(), AppError>
where
    D: HasIdProvider + ?Sized,
{
    let b2b_settings = build_b2b_settings(input.deployment_id);
    let (
        workspace_creator_role_id,
        workspace_member_role_id,
        org_creator_role_id,
        org_member_role_id,
    ) = next_b2b_role_ids(deps)?;
    DeploymentB2bBootstrapInsert::builder()
        .settings_row_id(next_id_from(deps)?)
        .workspace_creator_role_id(workspace_creator_role_id)
        .workspace_member_role_id(workspace_member_role_id)
        .org_creator_role_id(org_creator_role_id)
        .org_member_role_id(org_member_role_id)
        .b2b_settings(b2b_settings)
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    if let Some(social_connections_insert) =
        DeploymentSocialConnectionsBulkInsert::from_auth_methods(
            input.deployment_id,
            input.auth_methods,
            || next_id_from(deps),
        )?
    {
        social_connections_insert
            .execute_with_db(&mut *conn)
            .await?;
    }

    let console_id = console_deployment_id()?;
    ConsoleAppBootstrapInsert::builder()
        .console_deployment_id(console_id)
        .target_deployment_id(input.deployment_id)
        .event_catalog_slug(DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG)
        .build()?
        .execute_with_db(&mut *conn)
        .await?;

    Ok(())
}

pub(in crate::project) async fn bootstrap_deployment_defaults<D>(
    conn: &mut sqlx::PgConnection,
    deps: &D,
    input: DeploymentBootstrapInput<'_>,
) -> Result<(), AppError>
where
    D: HasIdProvider + ?Sized,
{
    insert_core_default_settings(conn, deps, &input).await?;
    insert_external_default_bootstraps(conn, deps, &input).await?;

    Ok(())
}

pub(in crate::project) async fn insert_staging_deployment_with_defaults<D>(
    conn: &mut sqlx::PgConnection,
    deps: &D,
    project_id: i64,
    app_name: String,
    auth_methods: &[String],
    key_material: DeploymentKeyMaterial,
) -> Result<StagingDeploymentInsertedRow, AppError>
where
    D: HasIdProvider + ?Sized,
{
    let hosts = build_staging_deployment_hosts();

    let deployment_row = StagingDeploymentInsert::builder()
        .id(next_id_from(deps)?)
        .project_id(project_id)
        .backend_host(hosts.backend_host)
        .frontend_host(hosts.frontend_host)
        .publishable_key(hosts.publishable_key)
        .mail_from_host("staging.wacht.services")
        .execute_with_db(&mut *conn)
        .await?;

    let waitlist_url = format!("https://{}/waitlist", deployment_row.frontend_host);
    bootstrap_deployment_defaults(
        conn,
        deps,
        DeploymentBootstrapInput {
            deployment_id: deployment_row.id,
            frontend_host: &deployment_row.frontend_host,
            app_name,
            auth_methods,
            waitlist_page_url: waitlist_url,
            support_page_url: "",
            key_material,
        },
    )
    .await?;

    Ok(deployment_row)
}

pub(in crate::project) async fn create_staging_deployment_for_project<D>(
    conn: &mut sqlx::PgConnection,
    deps: &D,
    project_id: i64,
    app_name: String,
    auth_methods: &[String],
    pulse_usage_disabled: bool,
    max_staging_deployments_per_project: i64,
) -> Result<StagingDeploymentInsertedRow, AppError>
where
    D: HasIdProvider + ?Sized,
{
    ProjectValidator::validate_auth_methods(auth_methods)?;
    ensure_phone_auth_allowed(auth_methods, pulse_usage_disabled)?;

    let staging_count = queries::StagingDeploymentCountByProjectQuery::builder()
        .project_id(project_id)
        .execute_with_db(conn.as_mut())
        .await?;

    let max_staging_deployments = positive_or_default(
        max_staging_deployments_per_project,
        MAX_STAGING_DEPLOYMENTS_PER_PROJECT,
    );

    if staging_count >= max_staging_deployments {
        return Err(AppError::BadRequest(format!(
            "Maximum of {} staging deployments allowed per project",
            max_staging_deployments
        )));
    }

    let key_material = generate_deployment_key_material().await?;

    insert_staging_deployment_with_defaults(
        conn,
        deps,
        project_id,
        app_name,
        auth_methods,
        key_material,
    )
    .await
}

pub(in crate::project) fn build_staging_deployment_model(
    deployment_row: StagingDeploymentInsertedRow,
) -> Deployment {
    Deployment {
        id: deployment_row.id,
        created_at: deployment_row.created_at,
        updated_at: deployment_row.updated_at,
        maintenance_mode: deployment_row.maintenance_mode,
        backend_host: deployment_row.backend_host,
        frontend_host: deployment_row.frontend_host,
        publishable_key: deployment_row.publishable_key,
        project_id: deployment_row.project_id,
        mode: DeploymentMode::from(deployment_row.mode),
        mail_from_host: deployment_row.mail_from_host,
        domain_verification_records: None,
        email_verification_records: None,
        email_provider: EmailProvider::default(),
        custom_smtp_config: None,
    }
}

pub(in crate::project) struct ProductionDeploymentModelInput {
    pub(in crate::project) id: i64,
    pub(in crate::project) created_at: chrono::DateTime<chrono::Utc>,
    pub(in crate::project) updated_at: chrono::DateTime<chrono::Utc>,
    pub(in crate::project) maintenance_mode: bool,
    pub(in crate::project) backend_host: String,
    pub(in crate::project) frontend_host: String,
    pub(in crate::project) publishable_key: String,
    pub(in crate::project) project_id: i64,
    pub(in crate::project) mode: String,
    pub(in crate::project) mail_from_host: String,
    pub(in crate::project) email_provider: String,
    pub(in crate::project) custom_smtp_config: Option<serde_json::Value>,
    pub(in crate::project) domain_verification_records: Option<DomainVerificationRecords>,
    pub(in crate::project) email_verification_records: Option<EmailVerificationRecords>,
}

pub(in crate::project) fn build_production_deployment_model(
    input: ProductionDeploymentModelInput,
) -> Result<Deployment, AppError> {
    Ok(Deployment {
        id: input.id,
        created_at: input.created_at,
        updated_at: input.updated_at,
        maintenance_mode: input.maintenance_mode,
        backend_host: input.backend_host,
        frontend_host: input.frontend_host,
        publishable_key: input.publishable_key,
        project_id: input.project_id,
        mode: DeploymentMode::from(input.mode),
        mail_from_host: input.mail_from_host,
        domain_verification_records: input.domain_verification_records,
        email_verification_records: input.email_verification_records,
        email_provider: EmailProvider::from(input.email_provider),
        custom_smtp_config: decode_public_custom_smtp_config(input.custom_smtp_config)?,
    })
}

pub(in crate::project) fn decode_public_custom_smtp_config(
    raw: Option<serde_json::Value>,
) -> Result<Option<CustomSmtpConfig>, AppError> {
    let decoded = raw
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| AppError::Internal(format!("Invalid custom_smtp_config JSON: {}", e)))?
        .map(|mut config: CustomSmtpConfig| {
            config.password = String::new();
            config
        });

    Ok(decoded)
}

pub(in crate::project) fn json_value<T: serde::Serialize>(
    value: &T,
) -> Result<serde_json::Value, AppError> {
    serde_json::to_value(value).map_err(|e| AppError::Serialization(e.to_string()))
}

pub(in crate::project) fn console_deployment_id() -> Result<i64, AppError> {
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
