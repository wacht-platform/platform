use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub logo_buffer: Vec<u8>,
    pub methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateStagingDeploymentRequest {
    pub auth_methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProductionDeploymentRequest {
    pub custom_domain: String,
    pub auth_methods: Vec<String>,
}