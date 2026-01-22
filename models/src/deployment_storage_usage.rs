use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentStorageUsage {
    pub deployment_id: i64,
    pub total_bytes: i64,
    pub updated_at: DateTime<Utc>,
    pub is_dirty: bool,
    pub created_at: DateTime<Utc>,
}
