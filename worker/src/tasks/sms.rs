use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Clone, Serialize, Deserialize)]
pub struct SMSTask {
    #[serde(rename = "type")]
    pub task_type: String,
    pub deployment_id: u64,
    pub phone_number: String,
}

use shared::state::AppState;

pub async fn send_sms_by_type(
    sms_type: &str,
    deployment_id: u64,
    _phone_number: &str,
    _app_state: &AppState,
) -> Result<String, String> {
    match sms_type {
        "verification" => Ok(format!("verification_sms_{}", deployment_id)),
        "otp" => Ok(format!("otp_sms_{}", deployment_id)),
        "alert" => Ok(format!("alert_sms_{}", deployment_id)),
        "notification" => Ok(format!("notification_sms_{}", deployment_id)),
        "welcome" => Ok(format!("welcome_sms_{}", deployment_id)),
        _ => {
            warn!("Unknown SMS type: {}", sms_type);
            Err(format!("Unknown SMS type: {}", sms_type))
        }
    }
}
