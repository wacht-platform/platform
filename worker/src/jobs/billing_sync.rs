use anyhow::Result;
use chrono::Datelike;
use commands::SyncBillingMetricsCommand;
use commands::{
    billing::{CompleteBillingSyncRunCommand, CreateBillingSyncRunCommand},
    pulse::DeductPulseCreditsCommand,
};
use common::{DodoClient, ReadConsistency, state::AppState};
use models::pulse_transaction::PulseTransactionType;
use queries::billing::{GetDeploymentProviderSubscriptionQuery, ProviderSubscriptionInfo};
use redis::AsyncCommands as _;
use tracing::{error, info, warn};

struct MetricConfig {
    event_name: &'static str,
    use_last_aggregation: bool,
}

fn get_metric_config(metric: &str) -> MetricConfig {
    match metric {
        "mau" => MetricConfig {
            event_name: "users.active",
            use_last_aggregation: true,
        },
        "mao" => MetricConfig {
            event_name: "organizations.active",
            use_last_aggregation: true,
        },
        "maw" => MetricConfig {
            event_name: "workspaces.active",
            use_last_aggregation: true,
        },
        "storage" => MetricConfig {
            event_name: "storage.used",
            use_last_aggregation: true,
        },
        "emails" => MetricConfig {
            event_name: "emails.total",
            use_last_aggregation: true,
        },
        "webhooks" => MetricConfig {
            event_name: "webhooks.total",
            use_last_aggregation: true,
        },
        "ai_token_input_cost_cents" => MetricConfig {
            event_name: "ai.token.input.cost",
            use_last_aggregation: false,
        },
        "ai_token_output_cost_cents" => MetricConfig {
            event_name: "ai.token.output.cost",
            use_last_aggregation: false,
        },
        "ai_search_queries" => MetricConfig {
            event_name: "ai.search.queries",
            use_last_aggregation: true,
        },
        "ai_search_query_cost_cents" => MetricConfig {
            event_name: "ai.search.query.cost",
            use_last_aggregation: false,
        },
        "sms_cost" => MetricConfig {
            event_name: "sms.cost",
            use_last_aggregation: false,
        },
        "api_checks" => MetricConfig {
            event_name: "api.checks.total",
            use_last_aggregation: true,
        },
        _ => MetricConfig {
            event_name: "unknown",
            use_last_aggregation: false,
        },
    }
}

pub async fn sync_redis_to_postgres_and_dodo(app_state: &AppState) -> Result<String> {
    info!("[BILLING SYNC] Starting Redis → PostgreSQL + Dodo sync");

    let mut redis = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await?;

    info!("[BILLING SYNC] Starting sync for current billing cycles");

    let sync_run_id = CreateBillingSyncRunCommand { from_event_id: 0 }
        .execute_with_db(app_state.db_router.writer())
        .await?;

    let dirty_key = format!(
        "billing:{}:dirty_deployments",
        format!(
            "{}-{:02}",
            chrono::Utc::now().year(),
            chrono::Utc::now().month()
        )
    );
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
        .execute_with_db(app_state.db_router.writer())
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
    .execute_with_db(app_state.db_router.writer())
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
    dirty_key: &str,
    dodo_client: Option<&DodoClient>,
) -> Result<i64> {
    let subscription_info = GetDeploymentProviderSubscriptionQuery::new(*deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Deployment {} has no active subscription - cannot determine billing period",
                deployment_id
            )
        })?;

    let billing_period_timestamp = subscription_info
        .previous_billing_date
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Deployment {} subscription missing previous_billing_date - cannot determine billing period",
                deployment_id
            )
        })?;

    let now = chrono::Utc::now();
    let current_month = format!("{}-{:02}", now.year(), now.month());

    let should_read_prev_month = now.day() <= 2;
    let prev_month = if should_read_prev_month {
        let prev = now - chrono::Duration::days(30);
        Some(format!("{}-{:02}", prev.year(), prev.month()))
    } else {
        None
    };

    let current_prefix = format!("billing:{}:deployment:{}", current_month, *deployment_id);

    let lua_script = r#"
        local prefix = ARGV[1]

        local mau_current = redis.call('PFCOUNT', prefix .. ':mau')
        local mao_current = redis.call('PFCOUNT', prefix .. ':mao')
        local maw_current = redis.call('PFCOUNT', prefix .. ':maw')
        local projects_current = redis.call('PFCOUNT', prefix .. ':projects')

        local metrics_key = prefix .. ':metrics'
        local emails_current = tonumber(redis.call('ZSCORE', metrics_key, 'emails') or 0)
        local webhooks_current = tonumber(redis.call('ZSCORE', metrics_key, 'webhooks') or 0)
        local ai_input_cost_current = tonumber(redis.call('ZSCORE', metrics_key, 'ai_token_input_cost_cents') or 0)
        local ai_output_cost_current = tonumber(redis.call('ZSCORE', metrics_key, 'ai_token_output_cost_cents') or 0)
        local ai_search_queries_current = tonumber(redis.call('ZSCORE', metrics_key, 'ai_search_queries') or 0)
        local ai_search_query_cost_current = tonumber(redis.call('ZSCORE', metrics_key, 'ai_search_query_cost_cents') or 0)
        local sms_cost_current = tonumber(redis.call('ZSCORE', metrics_key, 'sms_cost_cents') or 0)
        local api_checks_current = tonumber(redis.call('ZSCORE', metrics_key, 'api_checks') or 0)

        local last_synced_key = prefix .. ':last_synced'
        local mau_last = tonumber(redis.call('ZSCORE', last_synced_key, 'mau') or 0)
        local mao_last = tonumber(redis.call('ZSCORE', last_synced_key, 'mao') or 0)
        local maw_last = tonumber(redis.call('ZSCORE', last_synced_key, 'maw') or 0)
        local projects_last = tonumber(redis.call('ZSCORE', last_synced_key, 'projects') or 0)
        local emails_last = tonumber(redis.call('ZSCORE', last_synced_key, 'emails') or 0)
        local webhooks_last = tonumber(redis.call('ZSCORE', last_synced_key, 'webhooks') or 0)
        local ai_input_cost_last = tonumber(redis.call('ZSCORE', last_synced_key, 'ai_token_input_cost_cents') or redis.call('ZSCORE', last_synced_key, 'ai_token_input_cost') or 0)
        local ai_output_cost_last = tonumber(redis.call('ZSCORE', last_synced_key, 'ai_token_output_cost_cents') or redis.call('ZSCORE', last_synced_key, 'ai_token_output_cost') or 0)
        local ai_search_queries_last = tonumber(redis.call('ZSCORE', last_synced_key, 'ai_search_queries') or 0)
        local ai_search_query_cost_last = tonumber(redis.call('ZSCORE', last_synced_key, 'ai_search_query_cost_cents') or redis.call('ZSCORE', last_synced_key, 'ai_search_query_cost') or 0)
        local sms_cost_last = tonumber(redis.call('ZSCORE', last_synced_key, 'sms_cost') or redis.call('ZSCORE', last_synced_key, 'sms_cost_cents') or 0)
        local api_checks_last = tonumber(redis.call('ZSCORE', last_synced_key, 'api_checks') or 0)

        local mau_delta = mau_current - mau_last
        local mao_delta = mao_current - mao_last
        local maw_delta = maw_current - maw_last
        local projects_delta = projects_current - projects_last
        local emails_delta = emails_current - emails_last
        local webhooks_delta = webhooks_current - webhooks_last
        local ai_input_cost_delta = ai_input_cost_current - ai_input_cost_last
        local ai_output_cost_delta = ai_output_cost_current - ai_output_cost_last
        local ai_search_queries_delta = ai_search_queries_current - ai_search_queries_last
        local ai_search_query_cost_delta = ai_search_query_cost_current - ai_search_query_cost_last
        local sms_cost_delta = sms_cost_current - sms_cost_last
        local api_checks_delta = api_checks_current - api_checks_last

        return {
            tostring(mau_current), tostring(mau_delta),
            tostring(mao_current), tostring(mao_delta),
            tostring(maw_current), tostring(maw_delta),
            tostring(projects_current), tostring(projects_delta),
            tostring(emails_current), tostring(emails_delta),
            tostring(webhooks_current), tostring(webhooks_delta),
            tostring(ai_input_cost_current), tostring(ai_input_cost_delta),
            tostring(ai_output_cost_current), tostring(ai_output_cost_delta),
            tostring(ai_search_queries_current), tostring(ai_search_queries_delta),
            tostring(ai_search_query_cost_current), tostring(ai_search_query_cost_delta),
            tostring(sms_cost_current), tostring(sms_cost_delta),
            tostring(api_checks_current), tostring(api_checks_delta)
        }
    "#;

    let script = redis::Script::new(lua_script);
    let results: Vec<String> = script.arg(&current_prefix).invoke_async(redis).await?;
    let prev_results: Option<Vec<String>> = if let Some(prev_month_str) = &prev_month {
        let prev_prefix = format!("billing:{}:deployment:{}", prev_month_str, *deployment_id);
        Some(script.arg(&prev_prefix).invoke_async(redis).await?)
    } else {
        None
    };
    let aggregate = |current_idx: usize, prev_idx: usize| -> i64 {
        let current_val = results[current_idx].parse::<i64>().unwrap_or(0);
        let prev_val = prev_results
            .as_ref()
            .and_then(|r| r.get(prev_idx))
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        current_val + prev_val
    };

    let metrics = vec![
        ("mau", aggregate(0, 0), aggregate(1, 1)),
        ("mao", aggregate(2, 2), aggregate(3, 3)),
        ("maw", aggregate(4, 4), aggregate(5, 5)),
        ("projects", aggregate(6, 6), aggregate(7, 7)),
        ("emails", aggregate(8, 8), aggregate(9, 9)),
        ("webhooks", aggregate(10, 10), aggregate(11, 11)),
        (
            "ai_token_input_cost_cents",
            aggregate(12, 12),
            aggregate(13, 13),
        ),
        (
            "ai_token_output_cost_cents",
            aggregate(14, 14),
            aggregate(15, 15),
        ),
        ("ai_search_queries", aggregate(16, 16), aggregate(17, 17)),
        (
            "ai_search_query_cost_cents",
            aggregate(18, 18),
            aggregate(19, 19),
        ),
        ("sms_cost", aggregate(20, 20), aggregate(21, 21)),
    ];
    let mut total_units_synced = 0i64;
    let mut metrics_to_sync = Vec::new();

    for (metric_name, current, delta) in &metrics {
        if *delta <= 0 {
            continue;
        }
        if matches!(
            *metric_name,
            "ai_token_input_cost_cents"
                | "ai_token_output_cost_cents"
                | "ai_search_query_cost_cents"
                | "sms_cost"
        ) {
            let transaction_type = if metric_name.starts_with("ai") {
                PulseTransactionType::UsageAi
            } else {
                PulseTransactionType::UsageSms
            };

            let deduct_pulse_command = DeductPulseCreditsCommand {
                transaction_id: Some(app_state.sf.next_id()? as i64),
                owner_id: subscription_info.owner_id.clone(),
                amount_pulse_cents: *delta,
                transaction_type,
                reference_id: Some(app_state.sf.next_id().unwrap().to_string()),
            };
            match deduct_pulse_command
                .execute_with_deps(app_state)
                .await
            {
                Ok(_) => {
                    info!(
                        "[BILLING SYNC] Deducted {} Pulse cents for {} from deployment {}",
                        delta, metric_name, deployment_id
                    );

                    let last_synced_key = format!("{}:last_synced", current_prefix);
                    let (canonical_key, legacy_key): (&str, Option<&str>) = match *metric_name {
                        "ai_token_input_cost_cents" => {
                            ("ai_token_input_cost_cents", Some("ai_token_input_cost"))
                        }
                        "ai_token_output_cost_cents" => {
                            ("ai_token_output_cost_cents", Some("ai_token_output_cost"))
                        }
                        "ai_search_query_cost_cents" => {
                            ("ai_search_query_cost_cents", Some("ai_search_query_cost"))
                        }
                        "sms_cost" => ("sms_cost", Some("sms_cost_cents")),
                        _ => ("", None),
                    };

                    if !canonical_key.is_empty() {
                        let _: () = redis::cmd("ZADD")
                            .arg(&last_synced_key)
                            .arg(*current)
                            .arg(canonical_key)
                            .query_async(redis)
                            .await?;

                        if let Some(legacy_key) = legacy_key {
                            let _: () = redis::cmd("ZADD")
                                .arg(&last_synced_key)
                                .arg(*current)
                                .arg(legacy_key)
                                .query_async(redis)
                                .await?;
                        }

                        let _: () = redis::cmd("EXPIRE")
                            .arg(&last_synced_key)
                            .arg(5184000)
                            .query_async(redis)
                            .await?;
                    }
                }
                Err(e) => {
                    error!(
                        "[BILLING SYNC] Failed to deduct Pulse credits for deployment {}: {}",
                        deployment_id, e
                    );
                }
            }
            continue;
        }

        total_units_synced += delta;
        metrics_to_sync.push((metric_name.to_string(), *current));
    }

    let metrics_for_dodo = if !metrics_to_sync.is_empty() {
        SyncBillingMetricsCommand {
            deployment_id: *deployment_id,
            billing_account_id: subscription_info.billing_account_id.clone(),
            billing_period: billing_period_timestamp,
            metrics: metrics_to_sync,
            redis_prefix: current_prefix.clone(),
        }
        .execute_with_deps(app_state)
        .await?
    } else {
        Vec::new()
    };

    if total_units_synced > 0 {
        let _: () = redis
            .zincr(dirty_key, *deployment_id, -(total_units_synced as f64))
            .await?;
    }

    if let Some(dodo) = dodo_client {
        if !metrics_for_dodo.is_empty() {
            sync_to_dodo_with_data(
                dodo,
                app_state,
                *deployment_id,
                &metrics_for_dodo,
                &subscription_info,
            )
            .await;
        }
    }

    Ok(total_units_synced)
}

async fn sync_to_dodo_with_data(
    dodo_client: &DodoClient,
    _app_state: &AppState,
    deployment_id: i64,
    metrics_data: &[(String, i64)],
    subscription_info: &ProviderSubscriptionInfo,
) {
    let customer_id = &subscription_info.provider_customer_id;

    if subscription_info.plan_name == "starter" {
        return;
    }

    for (metric_name, current_value) in metrics_data {
        let config = get_metric_config(metric_name);
        if config.event_name == "unknown" {
            continue;
        }

        let event_id = format!(
            "{}_{}_{}",
            config.event_name,
            deployment_id,
            chrono::Utc::now().timestamp()
        );

        match dodo_client
            .ingest_usage_events(
                customer_id,
                config.event_name,
                *current_value,
                &event_id,
                config.use_last_aggregation,
            )
            .await
        {
            Ok(_) => {
                info!(
                    "[DODO] ✅ Synced {}={} for deployment {}",
                    config.event_name, current_value, deployment_id
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
