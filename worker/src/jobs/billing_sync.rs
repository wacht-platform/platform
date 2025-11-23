use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};
use commands::{
    Command,
    billing::{
        CompleteBillingSyncRunCommand, CreateBillingSyncRunCommand, UpsertUsageSnapshotCommand,
    },
};
use common::{DodoClient, state::AppState};
use queries::{GetDeploymentProviderSubscriptionQuery, Query};
use redis::AsyncCommands as _;
use tracing::{error, info, warn};

struct MetricConfig {
    event_name: &'static str,
    use_last_aggregation: bool,
}

fn get_metric_config(metric: &str) -> MetricConfig {
    match metric {
        "mau" => MetricConfig { event_name: "users.active", use_last_aggregation: true },
        "mao" => MetricConfig { event_name: "organizations.active", use_last_aggregation: true },
        "maw" => MetricConfig { event_name: "workspaces.active", use_last_aggregation: true },
        "storage" => MetricConfig { event_name: "storage.used", use_last_aggregation: false },
        "emails" => MetricConfig { event_name: "emails.sent", use_last_aggregation: false },
        "webhooks" => MetricConfig { event_name: "webhooks.sent", use_last_aggregation: false },
        "ai_tokens_input" => MetricConfig { event_name: "ai.tokens.input", use_last_aggregation: false },
        "ai_tokens_output" => MetricConfig { event_name: "ai.tokens.output", use_last_aggregation: false },
        "sms_cost" => MetricConfig { event_name: "sms.cost", use_last_aggregation: false },
        "api_checks" => MetricConfig { event_name: "api.checks", use_last_aggregation: false },
        _ => MetricConfig { event_name: "unknown", use_last_aggregation: false },
    }
}

pub async fn sync_redis_to_postgres_and_dodo(app_state: &AppState) -> Result<String> {
    info!("[BILLING SYNC] Starting Redis → PostgreSQL + Dodo sync");

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

    let dodo_client = match DodoClient::new() {
        Ok(client) => Some(client),
        Err(e) => {
            warn!(
                "[BILLING SYNC] Dodo not configured: {}. Syncing to PostgreSQL only.",
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
            dodo_client.as_ref(),
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

async fn sync_deployment(
    redis: &mut redis::aio::MultiplexedConnection,
    app_state: &AppState,
    deployment_id: &i64,
    billing_period: &str,
    billing_period_date: NaiveDate,
    dirty_key: &str,
    dodo_client: Option<&DodoClient>,
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
        local sms_cost_current = tonumber(redis.call('ZSCORE', metrics_key, 'sms_cost_cents') or 0)
        local storage_current = tonumber(redis.call('ZSCORE', metrics_key, 'storage_gb') or 0)
        local api_checks_current = tonumber(redis.call('ZSCORE', metrics_key, 'api_checks') or 0)

        local last_synced_key = prefix .. ':last_synced'
        local mau_last = tonumber(redis.call('ZSCORE', last_synced_key, 'mau') or 0)
        local mao_last = tonumber(redis.call('ZSCORE', last_synced_key, 'mao') or 0)
        local maw_last = tonumber(redis.call('ZSCORE', last_synced_key, 'maw') or 0)
        local projects_last = tonumber(redis.call('ZSCORE', last_synced_key, 'projects') or 0)
        local emails_last = tonumber(redis.call('ZSCORE', last_synced_key, 'emails') or 0)
        local webhooks_last = tonumber(redis.call('ZSCORE', last_synced_key, 'webhooks') or 0)
        local ai_input_last = tonumber(redis.call('ZSCORE', last_synced_key, 'ai_tokens_input') or 0)
        local ai_output_last = tonumber(redis.call('ZSCORE', last_synced_key, 'ai_tokens_output') or 0)
        local sms_cost_last = tonumber(redis.call('ZSCORE', last_synced_key, 'sms_cost_cents') or 0)
        local storage_last = tonumber(redis.call('ZSCORE', last_synced_key, 'storage_gb') or 0)
        local api_checks_last = tonumber(redis.call('ZSCORE', last_synced_key, 'api_checks') or 0)

        local mau_delta = mau_current - mau_last
        local mao_delta = mao_current - mao_last
        local maw_delta = maw_current - maw_last
        local projects_delta = projects_current - projects_last
        local emails_delta = emails_current - emails_last
        local webhooks_delta = webhooks_current - webhooks_last
        local ai_input_delta = ai_input_current - ai_input_last
        local ai_output_delta = ai_output_current - ai_output_last
        local sms_cost_delta = sms_cost_current - sms_cost_last
        local storage_delta = storage_current - storage_last
        local api_checks_delta = api_checks_current - api_checks_last

        redis.call('ZADD', last_synced_key, mau_current, 'mau')
        redis.call('ZADD', last_synced_key, mao_current, 'mao')
        redis.call('ZADD', last_synced_key, maw_current, 'maw')
        redis.call('ZADD', last_synced_key, projects_current, 'projects')
        redis.call('ZADD', last_synced_key, emails_current, 'emails')
        redis.call('ZADD', last_synced_key, webhooks_current, 'webhooks')
        redis.call('ZADD', last_synced_key, ai_input_current, 'ai_tokens_input')
        redis.call('ZADD', last_synced_key, ai_output_current, 'ai_tokens_output')
        redis.call('ZADD', last_synced_key, sms_cost_current, 'sms_cost_cents')
        redis.call('ZADD', last_synced_key, storage_current, 'storage_gb')
        redis.call('ZADD', last_synced_key, api_checks_current, 'api_checks')
        redis.call('EXPIRE', last_synced_key, 5184000)

        return {
            tostring(mau_current), tostring(mau_delta),
            tostring(mao_current), tostring(mao_delta),
            tostring(maw_current), tostring(maw_delta),
            tostring(projects_current), tostring(projects_delta),
            tostring(emails_current), tostring(emails_delta),
            tostring(webhooks_current), tostring(webhooks_delta),
            tostring(ai_input_current), tostring(ai_input_delta),
            tostring(ai_output_current), tostring(ai_output_delta),
            tostring(sms_cost_current), tostring(sms_cost_delta),
            tostring(storage_current), tostring(storage_delta),
            tostring(api_checks_current), tostring(api_checks_delta)
        }
    "#;

    let script = redis::Script::new(lua_script);
    let results: Vec<String> = script.arg(&prefix).invoke_async(redis).await?;

    let metrics = vec![
        ("mau", results[0].parse::<i64>().unwrap_or(0), results[1].parse::<i64>().unwrap_or(0)),
        ("mao", results[2].parse::<i64>().unwrap_or(0), results[3].parse::<i64>().unwrap_or(0)),
        ("maw", results[4].parse::<i64>().unwrap_or(0), results[5].parse::<i64>().unwrap_or(0)),
        ("projects", results[6].parse::<i64>().unwrap_or(0), results[7].parse::<i64>().unwrap_or(0)),
        ("emails", results[8].parse::<i64>().unwrap_or(0), results[9].parse::<i64>().unwrap_or(0)),
        ("webhooks", results[10].parse::<i64>().unwrap_or(0), results[11].parse::<i64>().unwrap_or(0)),
        ("ai_tokens_input", results[12].parse::<i64>().unwrap_or(0), results[13].parse::<i64>().unwrap_or(0)),
        ("ai_tokens_output", results[14].parse::<i64>().unwrap_or(0), results[15].parse::<i64>().unwrap_or(0)),
        ("sms_cost", results[16].parse::<i64>().unwrap_or(0), results[17].parse::<i64>().unwrap_or(0)),
        ("storage", results[18].parse::<i64>().unwrap_or(0), results[19].parse::<i64>().unwrap_or(0)),
        ("api_checks", results[20].parse::<i64>().unwrap_or(0), results[21].parse::<i64>().unwrap_or(0)),
    ];

    let mut total_units_synced = 0i64;

    for (metric_name, _current, delta) in &metrics {
        if *delta <= 0 {
            continue;
        }

        total_units_synced += delta;

        UpsertUsageSnapshotCommand {
            deployment_id: *deployment_id,
            billing_period: billing_period_date,
            metric_name: metric_name.to_string(),
            quantity: *delta,
            cost_cents: None,
        }
        .execute(app_state)
        .await?;
    }

    if let Some(dodo) = dodo_client {
        sync_to_dodo(dodo, app_state, *deployment_id, &metrics).await;
    }

    if total_units_synced > 0 {
        let _: () = redis
            .zincr(dirty_key, *deployment_id, -(total_units_synced as f64))
            .await?;
    }

    Ok(total_units_synced)
}

async fn sync_to_dodo(
    dodo_client: &DodoClient,
    app_state: &AppState,
    deployment_id: i64,
    metrics: &[(&str, i64, i64)],
) {
    let subscription_info = match GetDeploymentProviderSubscriptionQuery::new(deployment_id)
        .execute(app_state)
        .await
    {
        Ok(Some(info)) => info,
        Ok(None) => {
            info!(
                "[DODO] Deployment {} has no subscription configured",
                deployment_id
            );
            return;
        }
        Err(e) => {
            error!(
                "[DODO] Failed to fetch deployment {}: {}",
                deployment_id, e
            );
            return;
        }
    };

    let customer_id = subscription_info.provider_customer_id;

    for (metric_name, current, delta) in metrics {
        if *delta <= 0 {
            continue;
        }

        let config = get_metric_config(metric_name);
        if config.event_name == "unknown" {
            continue;
        }

        let value_to_send = if config.use_last_aggregation {
            *current
        } else {
            *delta
        };

        let event_id = format!(
            "{}_{}_{}",
            deployment_id,
            metric_name,
            chrono::Utc::now().timestamp_millis()
        );

        match dodo_client
            .ingest_usage_events(&customer_id, config.event_name, value_to_send, &event_id, config.use_last_aggregation)
            .await
        {
            Ok(_) => {
                info!(
                    "[DODO] ✅ Synced {}={} (delta={}) for deployment {}",
                    config.event_name, value_to_send, delta, deployment_id
                );
            }
            Err(e) => {
                error!(
                    "[DODO] ❌ Failed to sync {} for deployment {}: {}",
                    config.event_name, deployment_id, e
                );
            }
        }
    }
}
