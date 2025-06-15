use sqlx::Row;
use std::str::FromStr;

use super::Command;
use crate::{
    dto::json::{
        DeploymentAuthSettingsUpdates, DeploymentB2bSettingsUpdates,
        DeploymentDisplaySettingsUpdates, DeploymentRestrictionsUpdates,
        DeploymentSocialConnectionUpsert, NewDeploymentJwtTemplate, PartialDeploymentJwtTemplate,
    },
    error::AppError,
    models::{DeploymentJwtTemplate, DeploymentSocialConnection, SocialConnectionProvider},
    state::AppState,
};
use chrono::Utc;
use redis::AsyncCommands;
use serde_json::{Map, Value, json};

pub struct ClearDeploymentCacheCommand {
    pub deployment_id: i64,
}

impl ClearDeploymentCacheCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Command for ClearDeploymentCacheCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let deployment_row = sqlx::query!(
            "SELECT backend_host FROM deployments WHERE id = $1",
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let mut redis_conn = app_state
            .redis_client
            .get_multiplexed_tokio_connection()
            .await?;

        let cache_key = format!("deployment:{}", deployment_row.backend_host);
        let _: () = redis_conn.del(cache_key).await?;

        Ok(())
    }
}

pub struct UpdateDeploymentAuthSettingsCommand {
    pub deployment_id: i64,
    pub updates: DeploymentAuthSettingsUpdates,
}

impl UpdateDeploymentAuthSettingsCommand {
    pub fn new(deployment_id: i64, updates: DeploymentAuthSettingsUpdates) -> Self {
        Self {
            deployment_id,
            updates,
        }
    }
}

fn build_partial_json<T: serde::Serialize>(data: Option<&T>) -> Option<Value> {
    data.and_then(|d| match serde_json::to_value(d) {
        Ok(Value::Object(map)) => {
            let filtered_map: Map<String, Value> =
                map.into_iter().filter(|(_, v)| !v.is_null()).collect();
            if filtered_map.is_empty() {
                None
            } else {
                Some(Value::Object(filtered_map))
            }
        }
        Ok(_) => None,
        Err(_) => None,
    })
}

impl Command for UpdateDeploymentAuthSettingsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut text_updates: Vec<(&str, String)> = Vec::new();
        let mut int_updates: Vec<(&str, i64)> = Vec::new();
        let mut jsonb_merges: Vec<(&str, Value)> = Vec::new();

        if let Some(json_val) = build_partial_json(self.updates.email.as_ref()) {
            jsonb_merges.push(("email_address", json_val));
        }
        if let Some(json_val) = build_partial_json(self.updates.phone.as_ref()) {
            jsonb_merges.push(("phone_number", json_val));
        }
        if let Some(json_val) = build_partial_json(self.updates.username.as_ref()) {
            jsonb_merges.push(("username", json_val));
        }
        if let Some(json_val) = build_partial_json(self.updates.password.as_ref()) {
            jsonb_merges.push(("password", json_val));
        }
        if let Some(json_val) = build_partial_json(self.updates.backup_code.as_ref()) {
            jsonb_merges.push(("backup_code", json_val));
        }
        if let Some(json_val) = build_partial_json(self.updates.web3_wallet.as_ref()) {
            jsonb_merges.push(("web3_wallet", json_val));
        }

        if let Some(name_settings) = &self.updates.name {
            let mut first_name_partial = Map::new();
            if let Some(enabled) = name_settings.first_name_enabled {
                first_name_partial.insert("enabled".to_string(), json!(enabled));
            }
            if let Some(required) = name_settings.first_name_required {
                first_name_partial.insert("required".to_string(), json!(required));
            }
            if !first_name_partial.is_empty() {
                jsonb_merges.push(("first_name", Value::Object(first_name_partial)));
            }

            let mut last_name_partial = Map::new();
            if let Some(enabled) = name_settings.last_name_enabled {
                last_name_partial.insert("enabled".to_string(), json!(enabled));
            }
            if let Some(required) = name_settings.last_name_required {
                last_name_partial.insert("required".to_string(), json!(required));
            }
            if !last_name_partial.is_empty() {
                jsonb_merges.push(("last_name", Value::Object(last_name_partial)));
            }
        }

        let mut auth_factors_enabled_updates = Map::new();
        let mut process_auth_factors = false;
        if let Some(auth_factors) = &self.updates.authentication_factors {
            process_auth_factors = true;

            if let Some(json_val) = build_partial_json(auth_factors.magic_link.as_ref()) {
                jsonb_merges.push(("magic_link", json_val));
                if let Some(ml) = &auth_factors.magic_link {
                    if let Some(enabled) = ml.enabled {
                        auth_factors_enabled_updates
                            .insert("email_magic_link".to_string(), json!(enabled));
                    }
                }
            }
            if let Some(json_val) = build_partial_json(auth_factors.passkey.as_ref()) {
                jsonb_merges.push(("passkey", json_val));
                if let Some(pk) = &auth_factors.passkey {
                    if let Some(enabled) = pk.enabled {
                        auth_factors_enabled_updates.insert("passkey".to_string(), json!(enabled));
                    }
                }
            }

            if let Some(email_password) = auth_factors.email_password_enabled {
                auth_factors_enabled_updates
                    .insert("email_password".to_string(), json!(email_password));
            }
            if let Some(username_password) = auth_factors.username_password_enabled {
                auth_factors_enabled_updates
                    .insert("username_password".to_string(), json!(username_password));
            }
            if let Some(sso) = auth_factors.sso_enabled {
                auth_factors_enabled_updates.insert("sso".to_string(), json!(sso));
            }
            if let Some(web3_enabled) = auth_factors.web3_wallet_enabled {
                auth_factors_enabled_updates.insert("web3_wallet".to_string(), json!(web3_enabled));
            }
            if let Some(email_otp) = auth_factors.email_otp_enabled {
                auth_factors_enabled_updates.insert("email_otp".to_string(), json!(email_otp));
            }
            if let Some(phone_otp) = auth_factors.phone_otp_enabled {
                auth_factors_enabled_updates.insert("phone_otp".to_string(), json!(phone_otp));
            }

            if let Some(enabled) = auth_factors.second_factor_authenticator_enabled {
                auth_factors_enabled_updates.insert("authenticator".to_string(), json!(enabled));
            }
            if let Some(enabled) = auth_factors.second_factor_backup_code_enabled {
                auth_factors_enabled_updates.insert("backup_code".to_string(), json!(enabled));
            }
        }

        if process_auth_factors && !auth_factors_enabled_updates.is_empty() {
            jsonb_merges.push((
                "auth_factors_enabled",
                Value::Object(auth_factors_enabled_updates),
            ));
        }

        if let Some(policy) = self.updates.second_factor_policy {
            text_updates.push(("second_factor_policy", policy.to_string()));
        }

        if let Some(session) = &self.updates.multi_session_support {
            jsonb_merges.push(("multi_session_support", serde_json::to_value(session)?));
        }

        if let Some(session_token_lifetime) = &self.updates.session_token_lifetime {
            int_updates.push(("session_token_lifetime", *session_token_lifetime));
        }

        if let Some(session_validity_period) = &self.updates.session_validity_period {
            int_updates.push(("session_validity_period", *session_validity_period));
        }

        if let Some(session_inactive_timeout) = &self.updates.session_inactive_timeout {
            int_updates.push(("session_inactive_timeout", *session_inactive_timeout));
        }

        let has_text_updates = !text_updates.is_empty();
        let has_int_updates = !int_updates.is_empty();
        let has_jsonb_merges = !jsonb_merges.is_empty();

        if !has_text_updates && !has_int_updates && !has_jsonb_merges {
            println!(
                "No settings updates to apply for deployment_id: {}",
                self.deployment_id
            );
            return Ok(());
        }

        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_auth_settings SET updated_at = NOW() ");

        for (_, (column, value)) in text_updates.iter().enumerate() {
            query_builder.push(", ");
            query_builder.push(*column);
            query_builder.push(" = ");
            query_builder.push_bind(value);
        }

        for (_, (column, value)) in int_updates.iter().enumerate() {
            query_builder.push(", ");
            query_builder.push(*column);
            query_builder.push(" = ");
            query_builder.push_bind(value);
        }

        for (column, json_val) in jsonb_merges {
            query_builder.push(", ");
            let fragment = format!("{col} = COALESCE({col}, '{{}}'::jsonb) || ", col = column);
            query_builder.push(fragment);
            query_builder.push_bind(json_val);
            query_builder.push("::jsonb");
        }

        query_builder.push(" WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);

        query_builder
            .build()
            .execute(&app_state.db_pool)
            .await
            .unwrap();

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        Ok(())
    }
}

pub struct UpsertDeploymentSocialConnectionCommand {
    pub deployment_id: i64,
    pub connection: DeploymentSocialConnectionUpsert,
}

impl UpsertDeploymentSocialConnectionCommand {
    pub fn new(deployment_id: i64, connection: DeploymentSocialConnectionUpsert) -> Self {
        Self {
            deployment_id,
            connection,
        }
    }
}
impl Command for UpsertDeploymentSocialConnectionCommand {
    type Output = DeploymentSocialConnection;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            INSERT INTO deployment_social_connections (id, created_at, updated_at, deployment_id, provider, enabled, credentials)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (deployment_id, provider) DO UPDATE SET updated_at = NOW(), enabled = EXCLUDED.enabled, credentials = EXCLUDED.credentials RETURNING *
            "#,
            app_state.sf.next_id()? as i64,
            Utc::now(),
            Utc::now(),
            self.deployment_id,
            self.connection.provider.map(|p| String::from(p)),
            self.connection.enabled,
            serde_json::to_value(self.connection.credentials).unwrap(),
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let connection = DeploymentSocialConnection {
            id: result.id,
            created_at: result.created_at,
            updated_at: result.updated_at,
            deployment_id: result.deployment_id,
            provider: SocialConnectionProvider::from_str(&result.provider.unwrap()).ok(),
            enabled: result.enabled,
            credentials: serde_json::from_value(result.credentials.unwrap()).unwrap_or(None),
        };

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        Ok(connection)
    }
}

pub struct UpdateDeploymentRestrictionsCommand {
    pub deployment_id: i64,
    pub updates: DeploymentRestrictionsUpdates,
}

impl UpdateDeploymentRestrictionsCommand {
    pub fn new(deployment_id: i64, updates: DeploymentRestrictionsUpdates) -> Self {
        Self {
            deployment_id,
            updates,
        }
    }
}

impl Command for UpdateDeploymentRestrictionsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_restrictions SET updated_at = NOW() ");

        if let Some(allowlist_enabled) = self.updates.allowlist_enabled {
            query_builder.push(", allowlist_enabled = ");
            query_builder.push_bind(allowlist_enabled);
        }

        if let Some(blocklist_enabled) = self.updates.blocklist_enabled {
            query_builder.push(", blocklist_enabled = ");
            query_builder.push_bind(blocklist_enabled);
        }

        if let Some(block_subaddresses) = self.updates.block_subaddresses {
            query_builder.push(", block_subaddresses = ");
            query_builder.push_bind(block_subaddresses);
        }

        if let Some(block_disposable_emails) = self.updates.block_disposable_emails {
            query_builder.push(", block_disposable_emails = ");
            query_builder.push_bind(block_disposable_emails);
        }

        if let Some(block_voip_numbers) = self.updates.block_voip_numbers {
            query_builder.push(", block_voip_numbers = ");
            query_builder.push_bind(block_voip_numbers);
        }

        if let Some(country_restrictions) = self.updates.country_restrictions {
            query_builder.push(", country_restrictions = ");
            query_builder.push_bind(serde_json::to_value(country_restrictions)?);
        }

        if let Some(banned_keywords) = self.updates.banned_keywords {
            query_builder.push(", banned_keywords = ");
            query_builder.push_bind(banned_keywords);
        }

        if let Some(allowlisted_resources) = self.updates.allowlisted_resources {
            query_builder.push(", allowlisted_resources = ");
            query_builder.push_bind(allowlisted_resources);
        }

        if let Some(blocklisted_resources) = self.updates.blocklisted_resources {
            query_builder.push(", blocklisted_resources = ");
            query_builder.push_bind(blocklisted_resources);
        }

        if let Some(sign_up_mode) = self.updates.sign_up_mode {
            query_builder.push(", sign_up_mode = ");
            query_builder.push_bind(sign_up_mode.to_string());
        }

        if let Some(waitlist_collect_names) = self.updates.waitlist_collect_names {
            query_builder.push(", waitlist_collect_names = ");
            query_builder.push_bind(waitlist_collect_names);
        }

        query_builder.push(" WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);

        query_builder.build().execute(&app_state.db_pool).await?;

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        Ok(().into())
    }
}

pub struct CreateDeploymentJwtTemplateCommand {
    pub deployment_id: i64,
    pub template: NewDeploymentJwtTemplate,
}

impl CreateDeploymentJwtTemplateCommand {
    pub fn new(deployment_id: i64, template: NewDeploymentJwtTemplate) -> Self {
        Self {
            deployment_id,
            template,
        }
    }
}

impl Command for CreateDeploymentJwtTemplateCommand {
    type Output = DeploymentJwtTemplate;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            INSERT INTO deployment_jwt_templates (id, created_at, updated_at, deployment_id, name, token_lifetime, allowed_clock_skew, custom_signing_key, template)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
            "#,
            app_state.sf.next_id()? as i64,
            Utc::now(),
            Utc::now(),
            self.deployment_id,
            self.template.name,
            self.template.token_lifetime,
            self.template.allowed_clock_skew,
            serde_json::to_value(self.template.custom_signing_key).unwrap(),
            self.template.template,
        )
        .fetch_one(&app_state.db_pool)
        .await.unwrap();

        let template = DeploymentJwtTemplate {
            id: result.id,
            created_at: result.created_at,
            updated_at: result.updated_at,
            deployment_id: result.deployment_id,
            name: result.name,
            token_lifetime: result.token_lifetime,
            allowed_clock_skew: result.allowed_clock_skew,
            custom_signing_key: result
                .custom_signing_key
                .map(|v| serde_json::from_value(v).unwrap_or_default()),
            template: serde_json::from_value(result.template).unwrap_or_default(),
        };

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        Ok(template)
    }
}

pub struct UpdateDeploymentJwtTemplateCommand {
    pub id: i64,
    pub template: PartialDeploymentJwtTemplate,
}

impl UpdateDeploymentJwtTemplateCommand {
    pub fn new(id: i64, template: PartialDeploymentJwtTemplate) -> Self {
        Self { id, template }
    }
}

impl Command for UpdateDeploymentJwtTemplateCommand {
    type Output = DeploymentJwtTemplate;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_jwt_templates SET updated_at = NOW() ");

        if let Some(name) = &self.template.name {
            query_builder.push(", name = ");
            query_builder.push_bind(name);
        }

        if let Some(token_lifetime) = &self.template.token_lifetime {
            query_builder.push(", token_lifetime = ");
            query_builder.push_bind(token_lifetime);
        }

        if let Some(allowed_clock_skew) = &self.template.allowed_clock_skew {
            query_builder.push(", allowed_clock_skew = ");
            query_builder.push_bind(allowed_clock_skew);
        }

        if let Some(custom_signing_key) = &self.template.custom_signing_key {
            query_builder.push(", custom_signing_key = ");
            query_builder.push_bind(serde_json::to_value(custom_signing_key).unwrap());
        }

        if let Some(template) = &self.template.template {
            query_builder.push(", template = ");
            query_builder.push_bind(serde_json::to_value(template).unwrap());
        }

        query_builder.push(" WHERE id = ");
        query_builder.push_bind(self.id);

        query_builder.push(" RETURNING *");

        let result = query_builder.build().fetch_one(&app_state.db_pool).await?;

        let template = DeploymentJwtTemplate {
            id: result.get("id"),
            created_at: result.get("created_at"),
            updated_at: result.get("updated_at"),
            deployment_id: result.get("deployment_id"),
            name: result.get("name"),
            token_lifetime: result.get("token_lifetime"),
            allowed_clock_skew: result.get("allowed_clock_skew"),
            custom_signing_key: serde_json::from_value(result.get("custom_signing_key"))
                .unwrap_or_default(),
            template: result.get("template"),
        };

        let deployment_id: i64 = result.get("deployment_id");
        ClearDeploymentCacheCommand::new(deployment_id)
            .execute(app_state)
            .await?;

        Ok(template)
    }
}

pub struct DeleteDeploymentJwtTemplateCommand {
    pub id: i64,
}

impl DeleteDeploymentJwtTemplateCommand {
    pub fn new(id: i64) -> Self {
        Self { id }
    }
}

impl Command for DeleteDeploymentJwtTemplateCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let deployment_row = sqlx::query!(
            "SELECT deployment_id FROM deployment_jwt_templates WHERE id = $1",
            self.id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        sqlx::query("DELETE FROM deployment_jwt_templates WHERE id = $1")
            .bind(self.id)
            .execute(&app_state.db_pool)
            .await?;

        if let Some(row) = deployment_row {
            ClearDeploymentCacheCommand::new(row.deployment_id)
                .execute(app_state)
                .await?;
        }

        Ok(().into())
    }
}

pub struct UpdateDeploymentB2bSettingsCommand {
    deployment_id: i64,
    settings: DeploymentB2bSettingsUpdates,
}

impl UpdateDeploymentB2bSettingsCommand {
    pub fn new(deployment_id: i64, settings: DeploymentB2bSettingsUpdates) -> Self {
        Self {
            deployment_id,
            settings,
        }
    }
}

impl Command for UpdateDeploymentB2bSettingsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_b2b_settings SET updated_at = NOW() ");

        if let Some(organizations_enabled) = self.settings.organizations_enabled {
            query_builder.push(", organizations_enabled = ");
            query_builder.push_bind(organizations_enabled);
        }

        if let Some(workspaces_enabled) = self.settings.workspaces_enabled {
            query_builder.push(", workspaces_enabled = ");
            query_builder.push_bind(workspaces_enabled);
        }

        if let Some(ip_allowlist_per_org_enabled) = self.settings.ip_allowlist_per_org_enabled {
            query_builder.push(", ip_allowlist_per_org_enabled = ");
            query_builder.push_bind(ip_allowlist_per_org_enabled);
        }

        if let Some(max_allowed_org_members) = self.settings.max_allowed_org_members {
            query_builder.push(", max_allowed_org_members = ");
            query_builder.push_bind(max_allowed_org_members);
        }

        if let Some(max_allowed_workspace_members) = self.settings.max_allowed_workspace_members {
            query_builder.push(", max_allowed_workspace_members = ");
            query_builder.push_bind(max_allowed_workspace_members);
        }

        if let Some(allow_org_deletion) = self.settings.allow_org_deletion {
            query_builder.push(", allow_org_deletion = ");
            query_builder.push_bind(allow_org_deletion);
        }

        if let Some(allow_workspace_deletion) = self.settings.allow_workspace_deletion {
            query_builder.push(", allow_workspace_deletion = ");
            query_builder.push_bind(allow_workspace_deletion);
        }

        if let Some(custom_org_role_enabled) = self.settings.custom_org_role_enabled {
            query_builder.push(", custom_org_role_enabled = ");
            query_builder.push_bind(custom_org_role_enabled);
        }

        if let Some(custom_workspace_role_enabled) = self.settings.custom_workspace_role_enabled {
            query_builder.push(", custom_workspace_role_enabled = ");
            query_builder.push_bind(custom_workspace_role_enabled);
        }

        if let Some(default_workspace_creator_role_id) =
            self.settings.default_workspace_creator_role_id
        {
            query_builder.push(", default_workspace_creator_role_id = ");
            query_builder.push_bind(default_workspace_creator_role_id);
        }

        if let Some(default_workspace_member_role_id) =
            self.settings.default_workspace_member_role_id
        {
            query_builder.push(", default_workspace_member_role_id = ");
            query_builder.push_bind(default_workspace_member_role_id);
        }

        if let Some(default_org_creator_role_id) = self.settings.default_org_creator_role_id {
            query_builder.push(", default_org_creator_role_id = ");
            query_builder.push_bind(default_org_creator_role_id);
        }

        if let Some(default_org_member_role_id) = self.settings.default_org_member_role_id {
            query_builder.push(", default_org_member_role_id = ");
            query_builder.push_bind(default_org_member_role_id);
        }

        if let Some(limit_org_creation_per_user) = self.settings.limit_org_creation_per_user {
            query_builder.push(", limit_org_creation_per_user = ");
            query_builder.push_bind(limit_org_creation_per_user);
        }

        if let Some(allow_users_to_create_orgs) = self.settings.allow_users_to_create_orgs {
            query_builder.push(", allow_users_to_create_orgs = ");
            query_builder.push_bind(allow_users_to_create_orgs);
        }

        if let Some(limit_workspace_creation_per_org) =
            self.settings.limit_workspace_creation_per_org
        {
            query_builder.push(", limit_workspace_creation_per_org = ");
            query_builder.push_bind(limit_workspace_creation_per_org);
        }

        if let Some(org_creation_per_user_count) = self.settings.org_creation_per_user_count {
            query_builder.push(", org_creation_per_user_count = ");
            query_builder.push_bind(org_creation_per_user_count);
        }

        if let Some(workspaces_per_org_count) = self.settings.workspaces_per_org_count {
            query_builder.push(", workspaces_per_org_count = ");
            query_builder.push_bind(workspaces_per_org_count);
        }

        query_builder.push(" WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);

        let result = query_builder
            .build()
            .execute(&app_state.db_pool)
            .await
            .unwrap();

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "B2B settings for deployment {} not found",
                self.deployment_id
            )));
        }

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        Ok(())
    }
}

pub struct UpdateDeploymentDisplaySettingsCommand {
    deployment_id: i64,
    settings: DeploymentDisplaySettingsUpdates,
}

impl UpdateDeploymentDisplaySettingsCommand {
    pub fn new(deployment_id: i64, settings: DeploymentDisplaySettingsUpdates) -> Self {
        Self {
            deployment_id,
            settings,
        }
    }
}

impl Command for UpdateDeploymentDisplaySettingsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_ui_settings SET updated_at = NOW() ");

        if let Some(app_name) = &self.settings.app_name {
            query_builder.push(", app_name = ");
            query_builder.push_bind(app_name);
        }

        if let Some(tos_page_url) = &self.settings.tos_page_url {
            query_builder.push(", tos_page_url = ");
            query_builder.push_bind(tos_page_url);
        }

        if let Some(sign_in_page_url) = &self.settings.sign_in_page_url {
            query_builder.push(", sign_in_page_url = ");
            query_builder.push_bind(sign_in_page_url);
        }

        if let Some(sign_up_page_url) = &self.settings.sign_up_page_url {
            query_builder.push(", sign_up_page_url = ");
            query_builder.push_bind(sign_up_page_url);
        }

        if let Some(after_sign_out_one_page_url) = &self.settings.after_sign_out_one_page_url {
            query_builder.push(", after_sign_out_one_page_url = ");
            query_builder.push_bind(after_sign_out_one_page_url);
        }

        if let Some(after_sign_out_all_page_url) = &self.settings.after_sign_out_all_page_url {
            query_builder.push(", after_sign_out_all_page_url = ");
            query_builder.push_bind(after_sign_out_all_page_url);
        }

        if let Some(favicon_image_url) = &self.settings.favicon_image_url {
            query_builder.push(", favicon_image_url = ");
            query_builder.push_bind(favicon_image_url);
        }

        if let Some(logo_image_url) = &self.settings.logo_image_url {
            query_builder.push(", logo_image_url = ");
            query_builder.push_bind(logo_image_url);
        }

        if let Some(privacy_policy_url) = &self.settings.privacy_policy_url {
            query_builder.push(", privacy_policy_url = ");
            query_builder.push_bind(privacy_policy_url);
        }

        if let Some(signup_terms_statement) = &self.settings.signup_terms_statement {
            query_builder.push(", signup_terms_statement = ");
            query_builder.push_bind(signup_terms_statement);
        }

        if let Some(signup_terms_statement_shown) = &self.settings.signup_terms_statement_shown {
            query_builder.push(", signup_terms_statement_shown = ");
            query_builder.push_bind(signup_terms_statement_shown);
        }

        if let Some(light_mode_settings) = &self.settings.light_mode_settings {
            query_builder.push(", light_mode_settings = ");
            query_builder.push_bind(serde_json::to_value(light_mode_settings).unwrap());
        }

        if let Some(dark_mode_settings) = &self.settings.dark_mode_settings {
            query_builder.push(", dark_mode_settings = ");
            query_builder.push_bind(serde_json::to_value(dark_mode_settings).unwrap());
        }

        if let Some(after_logo_click_url) = &self.settings.after_logo_click_url {
            query_builder.push(", after_logo_click_url = ");
            query_builder.push_bind(after_logo_click_url);
        }

        if let Some(organization_profile_url) = &self.settings.organization_profile_url {
            query_builder.push(", organization_profile_url = ");
            query_builder.push_bind(organization_profile_url);
        }

        if let Some(create_organization_url) = &self.settings.create_organization_url {
            query_builder.push(", create_organization_url = ");
            query_builder.push_bind(create_organization_url);
        }

        if let Some(default_user_profile_image_url) = &self.settings.default_user_profile_image_url
        {
            query_builder.push(", default_user_profile_image_url = ");
            query_builder.push_bind(default_user_profile_image_url);
        }

        if let Some(default_organization_profile_image_url) =
            &self.settings.default_organization_profile_image_url
        {
            query_builder.push(", default_organization_profile_image_url = ");
            query_builder.push_bind(default_organization_profile_image_url);
        }

        if let Some(use_initials_for_user_profile_image) =
            &self.settings.use_initials_for_user_profile_image
        {
            query_builder.push(", use_initials_for_user_profile_image = ");
            query_builder.push_bind(use_initials_for_user_profile_image);
        }

        if let Some(use_initials_for_organization_profile_image) =
            &self.settings.use_initials_for_organization_profile_image
        {
            query_builder.push(", use_initials_for_organization_profile_image = ");
            query_builder.push_bind(use_initials_for_organization_profile_image);
        }

        if let Some(after_signup_redirect_url) = &self.settings.after_signup_redirect_url {
            query_builder.push(", after_signup_redirect_url = ");
            query_builder.push_bind(after_signup_redirect_url);
        }

        if let Some(after_signin_redirect_url) = &self.settings.after_signin_redirect_url {
            query_builder.push(", after_signin_redirect_url = ");
            query_builder.push_bind(after_signin_redirect_url);
        }

        if let Some(user_profile_url) = &self.settings.user_profile_url {
            query_builder.push(", user_profile_url = ");
            query_builder.push_bind(user_profile_url);
        }

        if let Some(after_create_organization_redirect_url) =
            &self.settings.after_create_organization_redirect_url
        {
            query_builder.push(", after_create_organization_redirect_url = ");
            query_builder.push_bind(after_create_organization_redirect_url);
        }

        if let Some(default_workspace_profile_image_url) =
            &self.settings.default_workspace_profile_image_url
        {
            query_builder.push(", default_workspace_profile_image_url = ");
            query_builder.push_bind(default_workspace_profile_image_url);
        }

        if let Some(waitlist_page_url) = &self.settings.waitlist_page_url {
            query_builder.push(", waitlist_page_url = ");
            query_builder.push_bind(waitlist_page_url);
        }

        if let Some(support_page_url) = &self.settings.support_page_url {
            query_builder.push(", support_page_url = ");
            query_builder.push_bind(support_page_url);
        }

        query_builder.push(" WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);

        let result = query_builder.build().execute(&app_state.db_pool).await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "Display settings for deployment {} not found",
                self.deployment_id
            )));
        }

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        Ok(().into())
    }
}
