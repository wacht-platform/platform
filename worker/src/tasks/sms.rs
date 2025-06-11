use celery::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};

#[derive(Clone, Serialize, Deserialize)]
pub struct SMSTask {
    #[serde(rename = "type")]
    pub task_type: String,
    pub deployment_id: u64,
    pub phone_number: String,
}

#[celery::task(name = "sms.send")]
pub async fn send_sms(task: SMSTask) -> TaskResult<String> {
    info!("SMS task: type={}, deployment_id={}, phone_number={}", task.task_type, task.deployment_id, task.phone_number);

    tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;

    match send_sms_by_type(&task.task_type, task.deployment_id, &task.phone_number).await {
        Ok(message_id) => {
            info!("SMS sent: type={}, phone_number={}, message_id={}", task.task_type, task.phone_number, message_id);
            Ok(format!("SMS sent: {}/{}/{}", task.task_type, task.phone_number, message_id))
        }
        Err(e) => {
            error!("SMS failed: type={}, phone_number={}, error={}", task.task_type, task.phone_number, e);
            Err(TaskError::UnexpectedError(format!("SMS sending failed: {}", e)))
        }
    }
}

async fn send_sms_by_type(sms_type: &str, deployment_id: u64, phone_number: &str) -> Result<String, String> {
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
