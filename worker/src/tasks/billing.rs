use chrono::{Datelike, Utc};
use common::state::AppState;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingEventTask {
    pub deployment_id: i64,
    pub event_type: String,
    pub resource_id: i64,
    #[serde(default)]
    pub cost_cents: Option<i64>, // For AI token costs, SMS costs, etc.
}

pub async fn process_billing_event(
    task: BillingEventTask,
    app_state: &AppState,
) -> Result<String, anyhow::Error> {
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await?;

    let now = Utc::now();
    let period = format!("{}-{:02}", now.year(), now.month());
    let prefix = format!("billing:{}:deployment:{}", period, task.deployment_id);

    let mut pipe = redis::pipe();
    pipe.atomic();

    match task.event_type.as_str() {
        "mau" => {
            pipe.pfadd(&format!("{}:mau", prefix), task.resource_id);
            pipe.expire(&format!("{}:mau", prefix), 5184000);
        }
        "organization_accessed" => {
            pipe.pfadd(&format!("{}:mao", prefix), task.resource_id);
            pipe.expire(&format!("{}:mao", prefix), 5184000);
        }
        "workspace_accessed" => {
            pipe.pfadd(&format!("{}:maw", prefix), task.resource_id);
            pipe.expire(&format!("{}:maw", prefix), 5184000);
        }
        "project_created" => {
            pipe.pfadd(&format!("{}:projects", prefix), task.resource_id);
            pipe.expire(&format!("{}:projects", prefix), 5184000);
        }
        "email_sent" => {
            pipe.zincr(&format!("{}:metrics", prefix), "emails", 1);
            pipe.expire(&format!("{}:metrics", prefix), 5184000);
        }
        "webhook_sent" => {
            pipe.zincr(&format!("{}:metrics", prefix), "webhooks", 1);
            pipe.expire(&format!("{}:metrics", prefix), 5184000);
        }
        "api_check" => {
            pipe.zincr(&format!("{}:metrics", prefix), "api_checks", 1);
            pipe.expire(&format!("{}:metrics", prefix), 5184000);
        }
        "sms_sent" => {
            let cost = task.cost_cents.unwrap_or(0);
            pipe.zincr(&format!("{}:metrics", prefix), "sms_cost_cents", cost);
            pipe.expire(&format!("{}:metrics", prefix), 5184000);
        }
        _ => {
            return Err(anyhow::anyhow!("Unknown event type: {}", task.event_type));
        }
    }

    pipe.zincr(
        &format!("billing:{}:dirty_deployments", period),
        task.deployment_id,
        1,
    );
    pipe.expire(&format!("billing:{}:dirty_deployments", period), 5184000);

    let _: () = pipe.query_async(&mut conn).await?;

    info!(
        "Billing event {} recorded for deployment {}",
        task.event_type, task.deployment_id
    );

    Ok(format!(
        "Recorded {} event for deployment {}",
        task.event_type, task.deployment_id
    ))
}
