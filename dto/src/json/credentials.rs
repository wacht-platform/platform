use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentCredentialsResponse {
    pub publishable_key: String,
    pub frontend_host: String,
    pub backend_host: String,
    pub api_key: DeploymentCredentialsApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentCredentialsApiKey {
    pub id: String,
    pub secret: String,
    pub prefix: String,
    pub suffix: String,
    pub app_slug: String,
}
