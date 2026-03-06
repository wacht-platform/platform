use anyhow::Result;
use commands::webhook_trigger::ReplayWebhookDeliveryCommand;
use common::error::AppError;
use common::state::AppState;
use dto::json::nats::WebhookReplayBatchPayload;
use redis::AsyncCommands;
use redis::Script;
use serde_json::Value;
use tokio::time::sleep;
use tracing::{error, info, warn};

pub async fn handle_webhook_replay_batch(app_state: &AppState, payload: Value) -> Result<String> {
    const MAX_ATTEMPTS_PER_DELIVERY: u8 = 3;
    const SNAPSHOT_TTL_SECS: i64 = 7200;

    let mut payload_value = payload;
    let task_id = payload_value
        .as_object_mut()
        .and_then(|obj| {
            obj.remove("__task_id")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
        })
        .unwrap_or_else(|| "unknown".to_string());

    let replay_payload: WebhookReplayBatchPayload = serde_json::from_value(payload_value)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize webhook replay payload: {}", e))?;

    let (deployment_id, app_slug, delivery_ids, effective_end_date_rfc3339) = match replay_payload {
        WebhookReplayBatchPayload::ByIds {
            deployment_id,
            app_slug,
            delivery_ids,
        } => {
            let deployment_id = deployment_id
                .parse::<i64>()
                .map_err(|e| anyhow::anyhow!("Invalid deployment_id in replay payload: {}", e))?;
            let ids: Vec<i64> = delivery_ids
                .iter()
                .filter_map(|s| s.parse::<i64>().ok())
                .collect();

            if ids.len() != delivery_ids.len() {
                error!(
                    "Some delivery IDs failed to parse. Original: {:?}, Parsed: {:?}",
                    delivery_ids, ids
                );
            }

            (deployment_id, app_slug, ids, None)
        }
        WebhookReplayBatchPayload::ByDateRange {
            deployment_id,
            app_slug,
            start_date,
            end_date,
            status,
            event_name,
            endpoint_id,
        } => {
            let deployment_id = deployment_id
                .parse::<i64>()
                .map_err(|e| anyhow::anyhow!("Invalid deployment_id in replay payload: {}", e))?;
            let snapshot_key = format!("worker:webhook:replay:{}:{}", app_slug, task_id);
            let mut resume_conn = app_state
                .redis_client
                .get_multiplexed_async_connection()
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to connect to Redis for replay resume: {}", e)
                })?;

            let persisted_end_raw: Option<String> = resume_conn
                .hget(&snapshot_key, "effective_end_date")
                .await
                .ok();
            let persisted_end = persisted_end_raw.as_deref().and_then(|raw| {
                chrono::DateTime::parse_from_rfc3339(raw)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .ok()
            });

            let effective_end_date = persisted_end.or(end_date).unwrap_or_else(chrono::Utc::now);

            if end_date.is_none() {
                info!(
                    "Replay by_date_range missing end_date; defaulting to current UTC: {}",
                    effective_end_date.to_rfc3339()
                );
            }
            let ids = app_state
                .clickhouse_service
                .get_deliveries_for_replay(
                    deployment_id,
                    app_slug.clone(),
                    start_date,
                    Some(effective_end_date),
                    status.as_deref(),
                    event_name.as_deref(),
                    endpoint_id,
                )
                .await?;

            (
                deployment_id,
                app_slug,
                ids,
                Some(effective_end_date.to_rfc3339()),
            )
        }
    };

    let snapshot_key = format!("worker:webhook:replay:{}:{}", app_slug, task_id);
    let active_count_key = format!("worker:webhook:replay:active_count:{}", app_slug);
    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Redis for replay snapshot: {}", e))?;

    let init_script = Script::new(
        r#"
        local key = KEYS[1]
        local task_id = ARGV[1]
        local app_slug = ARGV[2]
        local deployment_id = ARGV[3]
        local started_at = ARGV[4]
        local total_count = ARGV[5]
        local ttl = tonumber(ARGV[6])
        local effective_end_date = ARGV[7]
        local existing_status = redis.call('HGET', key, 'status')
        if existing_status == 'cancelled' then
          redis.call('EXPIRE', key, ttl)
          return 0
        end

        redis.call('HSETNX', key, 'task_id', task_id)
        redis.call('HSETNX', key, 'app_slug', app_slug)
        redis.call('HSETNX', key, 'deployment_id', deployment_id)
        redis.call('HSETNX', key, 'started_at', started_at)
        redis.call('HSETNX', key, 'processed_count', 0)
        redis.call('HSETNX', key, 'replayed_count', 0)
        redis.call('HSETNX', key, 'failed_count', 0)
        if effective_end_date and effective_end_date ~= '' then
          redis.call('HSETNX', key, 'effective_end_date', effective_end_date)
        end
        redis.call('HSET', key, 'status', 'running')
        redis.call('HSET', key, 'total_count', total_count)
        redis.call('EXPIRE', key, ttl)
        return 1
        "#,
    );
    let init_result: i32 = init_script
        .key(&snapshot_key)
        .arg(&task_id)
        .arg(&app_slug)
        .arg(deployment_id.to_string())
        .arg(chrono::Utc::now().to_rfc3339())
        .arg((delivery_ids.len() as i64).to_string())
        .arg(SNAPSHOT_TTL_SECS)
        .arg(effective_end_date_rfc3339.unwrap_or_default())
        .invoke_async(&mut redis_conn)
        .await?;
    if init_result == 0 {
        info!("Replay task {} is already cancelled before start", task_id);
        return Ok("Replay batch cancelled".to_string());
    }

    if delivery_ids.is_empty() {
        info!("No deliveries found to replay");
        let completion_script = Script::new(
            r#"
            local key = KEYS[1]
            local active_key = KEYS[2]
            redis.call('HSET', key, 'status', 'completed')
            redis.call('HSET', key, 'completed_at', ARGV[1])

            local reserved = redis.call('HGET', key, 'active_slot_reserved')
            if reserved == '1' then
              redis.call('HSET', key, 'active_slot_reserved', '0')
              local current_active = tonumber(redis.call('GET', active_key) or '0')
              if current_active > 0 then
                current_active = tonumber(redis.call('DECR', active_key))
              end
              if current_active <= 0 then
                redis.call('DEL', active_key)
              end
            end

            redis.call('EXPIRE', key, tonumber(ARGV[2]))
            return 1
            "#,
        );
        let _: i32 = completion_script
            .key(&snapshot_key)
            .key(&active_count_key)
            .arg(chrono::Utc::now().to_rfc3339())
            .arg(SNAPSHOT_TTL_SECS)
            .invoke_async(&mut redis_conn)
            .await?;
        return Ok("No deliveries found to replay".to_string());
    }

    let last_delivery_id: Option<i64> = redis_conn
        .hget(&snapshot_key, "last_delivery_id")
        .await
        .ok();
    let start_index = match last_delivery_id {
        Some(last_id) => delivery_ids
            .iter()
            .position(|id| *id == last_id)
            .map(|idx| idx + 1)
            .unwrap_or(0),
        None => 0,
    };

    if start_index > 0 {
        info!(
            "Resuming replay task {} from index {} (after delivery_id={})",
            task_id,
            start_index,
            last_delivery_id.unwrap_or_default()
        );
    }

    info!(
        "Found {} deliveries to replay for deployment {}",
        delivery_ids.len(),
        deployment_id
    );

    let mut was_cancelled = false;

    // Process each delivery with local retry; one failed ID should not halt the batch.
    for (idx, delivery_id) in delivery_ids.iter().enumerate().skip(start_index) {
        let current_status: Option<String> = redis_conn.hget(&snapshot_key, "status").await.ok();
        let cancelled_flag: Option<String> = redis_conn.hget(&snapshot_key, "cancelled").await.ok();
        if matches!(current_status.as_deref(), Some("cancelled"))
            || matches!(cancelled_flag.as_deref(), Some("1"))
        {
            info!(
                "Replay task {} cancelled at delivery {}/{}",
                task_id,
                idx + 1,
                delivery_ids.len()
            );
            was_cancelled = true;
            break;
        }

        info!(
            "Replay progress: {}/{} (delivery_id={})",
            idx + 1,
            delivery_ids.len(),
            delivery_id
        );

        let mut replayed = false;
        let mut last_error = String::new();

        for attempt in 1..=MAX_ATTEMPTS_PER_DELIVERY {
            let replay_command = ReplayWebhookDeliveryCommand {
                delivery_id: *delivery_id,
                deployment_id,
            };
            let result = replay_command
                .execute_with(
                    app_state.db_router.writer(),
                    &app_state.clickhouse_service,
                    &app_state.nats_client,
                    || Ok(app_state.sf.next_id()? as i64),
                )
                .await;

            match result {
                Ok(new_id) => {
                    info!(
                        "Successfully replayed delivery {} as new delivery {}",
                        delivery_id, new_id
                    );
                    replayed = true;
                    break;
                }
                Err(e) => {
                    last_error = e.to_string();
                    let should_retry = !matches!(
                        e,
                        AppError::BadRequest(_)
                            | AppError::NotFound(_)
                            | AppError::Forbidden(_)
                            | AppError::Unauthorized
                    );

                    if !should_retry {
                        warn!(
                            "Replay for delivery {} failed permanently (no retry): {}",
                            delivery_id, last_error
                        );
                        break;
                    }

                    if attempt < MAX_ATTEMPTS_PER_DELIVERY {
                        let backoff_ms = 200_u64 * (1_u64 << (attempt - 1));
                        warn!(
                            "Replay attempt {}/{} failed for delivery {}: {}. Retrying in {}ms",
                            attempt, MAX_ATTEMPTS_PER_DELIVERY, delivery_id, last_error, backoff_ms
                        );
                        sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    }
                }
            }
        }

        if !replayed {
            error!(
                "Failed to replay delivery {} after {} attempts: {}",
                delivery_id, MAX_ATTEMPTS_PER_DELIVERY, last_error
            );
        }

        let progress_script = Script::new(
            r#"
            local key = KEYS[1]
            local last_delivery_id = ARGV[1]
            local replay_delta = tonumber(ARGV[2])
            local failed_delta = tonumber(ARGV[3])
            local ttl = tonumber(ARGV[4])
            local existing_status = redis.call('HGET', key, 'status')
            if existing_status == 'cancelled' then
              redis.call('EXPIRE', key, ttl)
              return 0
            end

            redis.call('HSET', key, 'status', 'running')
            redis.call('HSET', key, 'last_delivery_id', last_delivery_id)
            redis.call('HINCRBY', key, 'processed_count', 1)
            if replay_delta > 0 then
              redis.call('HINCRBY', key, 'replayed_count', replay_delta)
            end
            if failed_delta > 0 then
              redis.call('HINCRBY', key, 'failed_count', failed_delta)
            end
            redis.call('EXPIRE', key, ttl)
            return 1
            "#,
        );
        let replay_delta = if replayed { 1 } else { 0 };
        let failed_delta = if replayed { 0 } else { 1 };
        let progress_result: i32 = progress_script
            .key(&snapshot_key)
            .arg(delivery_id.to_string())
            .arg(replay_delta)
            .arg(failed_delta)
            .arg(SNAPSHOT_TTL_SECS)
            .invoke_async(&mut redis_conn)
            .await?;
        if progress_result == 0 {
            was_cancelled = true;
            break;
        }
    }

    let completion_script = Script::new(
        r#"
        local key = KEYS[1]
        local active_key = KEYS[2]
        local completed_at = ARGV[1]
        local ttl = tonumber(ARGV[2])
        local existing_status = redis.call('HGET', key, 'status')
        if existing_status == 'cancelled' then
          redis.call('HSETNX', key, 'cancelled', '1')
          redis.call('HSETNX', key, 'cancelled_at', completed_at)
          redis.call('HSET', key, 'completed_at', completed_at)
          redis.call('EXPIRE', key, ttl)
          return 'cancelled'
        end
        redis.call('HSET', key, 'status', 'completed')
        redis.call('HSET', key, 'completed_at', completed_at)

        local reserved = redis.call('HGET', key, 'active_slot_reserved')
        if reserved == '1' then
          redis.call('HSET', key, 'active_slot_reserved', '0')
          local current_active = tonumber(redis.call('GET', active_key) or '0')
          if current_active > 0 then
            current_active = tonumber(redis.call('DECR', active_key))
          end
          if current_active <= 0 then
            redis.call('DEL', active_key)
          end
        end

        redis.call('EXPIRE', key, ttl)
        return 'completed'
        "#,
    );
    let final_status: String = completion_script
        .key(&snapshot_key)
        .key(&active_count_key)
        .arg(chrono::Utc::now().to_rfc3339())
        .arg(SNAPSHOT_TTL_SECS)
        .invoke_async(&mut redis_conn)
        .await?;

    let replayed_count: i64 = redis_conn
        .hget(&snapshot_key, "replayed_count")
        .await
        .unwrap_or(0);
    let failed_count: i64 = redis_conn
        .hget(&snapshot_key, "failed_count")
        .await
        .unwrap_or(0);

    if final_status == "cancelled" || was_cancelled {
        Ok(format!(
            "Replay batch cancelled: {} successful, {} failed",
            replayed_count, failed_count
        ))
    } else {
        Ok(format!(
            "Replay batch completed: {} successful, {} failed",
            replayed_count, failed_count
        ))
    }
}
