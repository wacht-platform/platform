use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentUsageRecord {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub subscription_id: Option<i64>,
    pub metric_name: String,
    pub quantity: i64,
    pub timestamp: DateTime<Utc>,
    pub stripe_usage_record_id: Option<String>,
    pub billing_period_start: DateTime<Utc>,
    pub billing_period_end: DateTime<Utc>,
    pub metadata: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UsageMetricSummary {
    pub metric_name: String,
    pub total_quantity: i64,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub record_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentUsageSummary {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub metrics: Vec<UsageMetricSummary>,
    pub total_records: i64,
}

impl DeploymentUsageRecord {
    pub fn is_in_current_billing_period(&self) -> bool {
        let now = Utc::now();
        now >= self.billing_period_start && now <= self.billing_period_end
    }

    pub fn is_synced_to_stripe(&self) -> bool {
        self.stripe_usage_record_id.is_some()
    }
}

impl UsageMetricSummary {
    pub fn average_per_day(&self) -> f64 {
        let days = (self.period_end - self.period_start).num_days().max(1) as f64;
        self.total_quantity as f64 / days
    }

    pub fn average_per_record(&self) -> f64 {
        if self.record_count > 0 {
            self.total_quantity as f64 / self.record_count as f64
        } else {
            0.0
        }
    }
}