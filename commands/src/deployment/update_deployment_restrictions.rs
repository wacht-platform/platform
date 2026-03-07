use common::{HasDbRouter, HasRedis, error::AppError};
use dto::json::DeploymentRestrictionsUpdates;

use super::ClearDeploymentCacheCommand;

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

impl UpdateDeploymentRestrictionsCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasRedis,
    {
        let mut conn = deps.db_router().writer().acquire().await?;
        let redis_client = deps.redis_client();
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

        query_builder.build().execute(&mut *conn).await?;

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute_with_conn_and_redis(&mut conn, redis_client)
            .await?;

        Ok(())
    }
}
