use common::{HasDbRouter, HasRedisProvider, error::AppError};
use dto::json::DeploymentAuthSettingsUpdates;
use serde_json::{Map, Value, json};

use super::ClearDeploymentCacheCommand;

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

fn enables_phone_auth(updates: &DeploymentAuthSettingsUpdates) -> bool {
    updates
        .phone
        .as_ref()
        .map(|phone| {
            phone.enabled == Some(true)
                || phone.verify_signup == Some(true)
                || phone.sms_verification_allowed == Some(true)
        })
        .unwrap_or(false)
        || updates
            .authentication_factors
            .as_ref()
            .and_then(|factors| factors.phone_otp_enabled)
            == Some(true)
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

impl UpdateDeploymentAuthSettingsCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasRedisProvider,
    {
        let writer = deps.db_router().writer();
        if enables_phone_auth(&self.updates) {
            let deployment = sqlx::query!(
                r#"
                SELECT
                    d.mode,
                    COALESCE(ba.pulse_usage_disabled, false) AS "pulse_usage_disabled!"
                FROM deployments d
                JOIN projects p ON p.id = d.project_id
                JOIN billing_accounts ba ON ba.id = p.billing_account_id
                WHERE d.id = $1 AND d.deleted_at IS NULL
                "#,
                self.deployment_id
            )
            .fetch_optional(writer)
            .await?
            .ok_or_else(|| AppError::NotFound("Deployment not found".to_string()))?;

            if deployment.mode.eq_ignore_ascii_case("staging") && deployment.pulse_usage_disabled {
                return Err(AppError::Validation(
                    "Prepaid recharge is required before enabling phone authentication for staging deployments".to_string(),
                ));
            }
        }

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

        if let Some(factor) = self.updates.first_factor {
            text_updates.push(("first_factor", factor.to_string()));
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
            return Ok(());
        }

        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_auth_settings SET updated_at = NOW() ");

        for (column, value) in &text_updates {
            query_builder.push(", ");
            query_builder.push(*column);
            query_builder.push(" = ");
            query_builder.push_bind(value);
        }

        for (column, value) in &int_updates {
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

        query_builder.build().execute(writer).await?;

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;

        Ok(())
    }
}
