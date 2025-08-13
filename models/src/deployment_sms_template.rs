use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct DeploymentSmsTemplate {
    pub id: i64,
    pub deployment_id: i64,
    pub reset_password_code_template: String,
    pub verification_code_template: String,
    pub password_change_template: String,
    pub password_remove_template: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Default for DeploymentSmsTemplate {
    fn default() -> Self {
        Self {
            id: 0,
            deployment_id: 0,
            reset_password_code_template: "Your {{app_name}} password reset code is: {{code}}"
                .to_string(),
            verification_code_template: "Your {{app_name}} verification code is: {{code}}"
                .to_string(),
            password_change_template: "Your {{app_name}} password has been changed".to_string(),
            password_remove_template: "Your {{app_name}} password has been removed".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
