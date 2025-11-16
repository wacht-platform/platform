use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};
use commands::{
    Command,
    billing::{
        CompleteBillingSyncRunCommand, CreateBillingSyncRunCommand, UpsertUsageSnapshotCommand,
    },
};
use common::{ChargebeeClient, state::AppState};
use queries::{GetDeploymentChargebeeSubscriptionIdQuery, Query};
use redis::AsyncCommands as _;
use rust_decimal::Decimal;
use std::str::FromStr;
use tracing::{error, info, warn};

/// Job: Sync Redis → PostgreSQL + Chargebee
///
/// This job runs periodically (every 5-15 minutes) to:
/// 1. Get dirty deployments from Redis (those with billing activity)
/// 2. Calculate deltas (current usage - last synced usage)
/// 3. Sync deltas to PostgreSQL and Chargebee
/// 4. Clear dirty deployments
///
/// Uses Lua script for atomic read-calculate-update operations
pub async fn sync_redis_to_postgres_and_chargebee(app_state: &AppState) -> Result<String> {
    info!("[BILLING SYNC] Starting Redis → PostgreSQL + Chargebee sync");

    let mut redis = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await?;

    let now = Utc::now();
    let billing_period = format!("{}-{:02}", now.year(), now.month());
    let billing_period_date = NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap();

    info!("[BILLING SYNC] Billing period: {}", billing_period);

    let sync_run_id = CreateBillingSyncRunCommand { from_event_id: 0 }
        .execute(app_state)
        .await?;

    let dirty_key = format!("billing:{}:dirty_deployments", billing_period);
    let dirty: Vec<(i64, f64)> = redis
        .zrangebyscore_withscores(&dirty_key, 1.0, f64::MAX)
        .await?;

    if dirty.is_empty() {
        info!("[BILLING SYNC] No dirty deployments to sync");
        CompleteBillingSyncRunCommand {
            sync_run_id,
            events_processed: 0,
            deployments_affected: 0,
        }
        .execute(app_state)
        .await?;
        return Ok("No dirty deployments".to_string());
    }

    info!("[BILLING SYNC] Found {} dirty deployments", dirty.len());

    let mut total_units_synced = 0i64;

    let chargebee_client = match ChargebeeClient::new() {
        Ok(client) => Some(client),
        Err(e) => {
            warn!(
                "[BILLING SYNC] Chargebee not configured or failed to initialize: {}. Syncing to PostgreSQL only.",
                e
            );
            None
        }
    };

    for (deployment_id, _event_count) in &dirty {
        match sync_deployment(
            &mut redis,
            app_state,
            deployment_id,
            &billing_period,
            billing_period_date,
            &dirty_key,
            chargebee_client.as_ref(),
        )
        .await
        {
            Ok(units) => {
                total_units_synced += units;
                info!(
                    "[BILLING SYNC] ✅ Synced deployment {} ({} units)",
                    deployment_id, units
                );
            }
            Err(e) => {
                error!(
                    "[BILLING SYNC] ❌ Failed to sync deployment {}: {}",
                    deployment_id, e
                );
            }
        }
    }

    CompleteBillingSyncRunCommand {
        sync_run_id,
        events_processed: total_units_synced,
        deployments_affected: dirty.len() as i32,
    }
    .execute(app_state)
    .await?;

    info!(
        "[BILLING SYNC] ✅ Completed sync of {} deployments ({} total units)",
        dirty.len(),
        total_units_synced
    );

    Ok(format!(
        "Synced {} deployments ({} units)",
        dirty.len(),
        total_units_synced
    ))
}

/// Sync a single deployment
async fn sync_deployment(
    redis: &mut redis::aio::MultiplexedConnection,
    app_state: &AppState,
    deployment_id: &i64,
    billing_period: &str,
    billing_period_date: NaiveDate,
    dirty_key: &str,
    chargebee_client: Option<&ChargebeeClient>,
) -> Result<i64> {
    let prefix = format!("billing:{}:deployment:{}", billing_period, *deployment_id);

    let lua_script = r#"
        local prefix = ARGV[1]

        local mau_current = redis.call('PFCOUNT', prefix .. ':mau')
        local mao_current = redis.call('PFCOUNT', prefix .. ':mao')
        local maw_current = redis.call('PFCOUNT', prefix .. ':maw')
        local projects_current = redis.call('PFCOUNT', prefix .. ':projects')

        local metrics_key = prefix .. ':metrics'
        local emails_current = tonumber(redis.call('ZSCORE', metrics_key, 'emails') or 0)
        local webhooks_current = tonumber(redis.call('ZSCORE', metrics_key, 'webhooks') or 0)
        local ai_input_current = tonumber(redis.call('ZSCORE', metrics_key, 'ai_tokens_input') or 0)
        local ai_output_current = tonumber(redis.call('ZSCORE', metrics_key, 'ai_tokens_output') or 0)
        local sms_count_current = tonumber(redis.call('ZSCORE', metrics_key, 'sms_count') or 0)
        local sms_cost_current = tonumber(redis.call('ZSCORE', metrics_key, 'sms_cost_cents') or 0)

        local last_synced_key = prefix .. ':last_synced'
        local mau_last = tonumber(redis.call('ZSCORE', last_synced_key, 'mau') or 0)
        local mao_last = tonumber(redis.call('ZSCORE', last_synced_key, 'mao') or 0)
        local maw_last = tonumber(redis.call('ZSCORE', last_synced_key, 'maw') or 0)
        local projects_last = tonumber(redis.call('ZSCORE', last_synced_key, 'projects') or 0)
        local emails_last = tonumber(redis.call('ZSCORE', last_synced_key, 'emails') or 0)
        local webhooks_last = tonumber(redis.call('ZSCORE', last_synced_key, 'webhooks') or 0)
        local ai_input_last = tonumber(redis.call('ZSCORE', last_synced_key, 'ai_tokens_input') or 0)
        local ai_output_last = tonumber(redis.call('ZSCORE', last_synced_key, 'ai_tokens_output') or 0)
        local sms_count_last = tonumber(redis.call('ZSCORE', last_synced_key, 'sms_count') or 0)
        local sms_cost_last = tonumber(redis.call('ZSCORE', last_synced_key, 'sms_cost_cents') or 0)

        local mau_delta = mau_current - mau_last
        local mao_delta = mao_current - mao_last
        local maw_delta = maw_current - maw_last
        local projects_delta = projects_current - projects_last
        local emails_delta = emails_current - emails_last
        local webhooks_delta = webhooks_current - webhooks_last
        local ai_input_delta = ai_input_current - ai_input_last
        local ai_output_delta = ai_output_current - ai_output_last
        local sms_count_delta = sms_count_current - sms_count_last
        local sms_cost_delta = sms_cost_current - sms_cost_last

        redis.call('ZADD', last_synced_key, mau_current, 'mau')
        redis.call('ZADD', last_synced_key, mao_current, 'mao')
        redis.call('ZADD', last_synced_key, maw_current, 'maw')
        redis.call('ZADD', last_synced_key, projects_current, 'projects')
        redis.call('ZADD', last_synced_key, emails_current, 'emails')
        redis.call('ZADD', last_synced_key, webhooks_current, 'webhooks')
        redis.call('ZADD', last_synced_key, ai_input_current, 'ai_tokens_input')
        redis.call('ZADD', last_synced_key, ai_output_current, 'ai_tokens_output')
        redis.call('ZADD', last_synced_key, sms_count_current, 'sms_count')
        redis.call('ZADD', last_synced_key, sms_cost_current, 'sms_cost_cents')
        redis.call('EXPIRE', last_synced_key, 5184000)

        return {
            tostring(mau_delta),
            tostring(mao_delta),
            tostring(maw_delta),
            tostring(projects_delta),
            tostring(emails_delta),
            tostring(webhooks_delta),
            tostring(ai_input_delta),
            tostring(ai_output_delta),
            tostring(sms_count_delta),
            tostring(sms_cost_delta)
        }
    "#;

    let script = redis::Script::new(lua_script);
    let results: Vec<String> = script.arg(&prefix).invoke_async(redis).await?;

    let mau_delta: i64 = results[0].parse()?;
    let mao_delta: i64 = results[1].parse()?;
    let maw_delta: i64 = results[2].parse()?;
    let projects_delta: i64 = results[3].parse()?;
    let emails_delta: i64 = results[4].parse()?;
    let webhooks_delta: i64 = results[5].parse()?;
    let ai_input_delta: i64 = results[6].parse()?;
    let ai_output_delta: i64 = results[7].parse()?;
    let sms_count_delta: i64 = results[8].parse()?;
    let sms_cost_delta: Decimal = Decimal::from_str(&results[9])?;

    let mut total_units_synced = 0i64;

    if mau_delta > 0 {
        total_units_synced += mau_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "mau".to_string(),
            quantity: mau_delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if mao_delta > 0 {
        total_units_synced += mao_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "mao".to_string(),
            quantity: mao_delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if maw_delta > 0 {
        total_units_synced += maw_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "maw".to_string(),
            quantity: maw_delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if projects_delta > 0 {
        total_units_synced += projects_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "projects".to_string(),
            quantity: projects_delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if emails_delta > 0 {
        total_units_synced += emails_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "emails".to_string(),
            quantity: emails_delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if webhooks_delta > 0 {
        total_units_synced += webhooks_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "webhooks".to_string(),
            quantity: webhooks_delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if ai_input_delta > 0 {
        total_units_synced += ai_input_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "ai_tokens_input".to_string(),
            quantity: ai_input_delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if ai_output_delta > 0 {
        total_units_synced += ai_output_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "ai_tokens_output".to_string(),
            quantity: ai_output_delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if sms_count_delta > 0 {
        total_units_synced += sms_count_delta;
        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: "sms".to_string(),
            quantity: sms_count_delta,
            cost_cents: Some(sms_cost_delta),
        }
        .execute(app_state)
        .await?;
    }

    if let Some(cb_client) = chargebee_client {
        sync_to_chargebee(
            cb_client,
            app_state,
            *deployment_id,
            billing_period_date,
            mau_delta,
            mao_delta,
            maw_delta,
            projects_delta,
            emails_delta,
            webhooks_delta,
            sms_count_delta,
        )
        .await;
    }

    if total_units_synced > 0 {
        let _: () = redis
            .zincr(dirty_key, *deployment_id, -(total_units_synced as f64))
            .await?;

        let _: f64 = redis
            .zscore::<&str, i64, f64>(dirty_key, *deployment_id)
            .await?;
    }

    Ok(total_units_synced)
}

/// Sync usage metrics to Chargebee
async fn sync_to_chargebee(
    chargebee_client: &ChargebeeClient,
    app_state: &AppState,
    deployment_id: i64,
    billing_period: NaiveDate,
    mau_delta: i64,
    mao_delta: i64,
    maw_delta: i64,
    projects_delta: i64,
    emails_delta: i64,
    webhooks_delta: i64,
    sms_delta: i64,
) {
    let subscription_id = match GetDeploymentChargebeeSubscriptionIdQuery::new(deployment_id)
        .execute(app_state)
        .await
    {
        Ok(Some(sub_id)) => sub_id,
        Ok(None) => {
            info!(
                "[CHARGEBEE] Deployment {} has no Chargebee subscription configured",
                deployment_id
            );
            return;
        }
        Err(e) => {
            error!(
                "[CHARGEBEE] Failed to fetch deployment {}: {}",
                deployment_id, e
            );
            return;
        }
    };

    let usage_date = billing_period
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();

    let metrics = vec![
        ("mau", mau_delta),
        ("mao", mao_delta),
        ("maw", maw_delta),
        ("projects", projects_delta),
        ("emails", emails_delta),
        ("webhooks", webhooks_delta),
        ("sms", sms_delta),
    ];

    for (metric_name, delta) in metrics {
        if delta > 0 {
            match chargebee_client
                .record_usage(&subscription_id, metric_name, delta, Some(usage_date))
                .await
            {
                Ok(_) => {
                    info!(
                        "[CHARGEBEE] ✅ Synced {} {} for deployment {}",
                        delta, metric_name, deployment_id
                    );
                }
                Err(e) => {
                    error!(
                        "[CHARGEBEE] ❌ Failed to sync {} for deployment {}: {}",
                        metric_name, deployment_id, e
                    );
                }
            }
        }
    }
}
