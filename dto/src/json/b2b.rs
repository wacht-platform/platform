use serde::Deserialize;

// Organization models
#[derive(Deserialize)]
pub struct CreateOrganizationRequest {
    pub name: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub public_metadata: Option<serde_json::Value>,
    pub private_metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct UpdateOrganizationRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub public_metadata: Option<serde_json::Value>,
    pub private_metadata: Option<serde_json::Value>,
}

// Workspace models
#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub public_metadata: Option<serde_json::Value>,
    pub private_metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct UpdateWorkspaceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub public_metadata: Option<serde_json::Value>,
    pub private_metadata: Option<serde_json::Value>,
}

// Organization member models
#[derive(Deserialize)]
pub struct AddOrganizationMemberRequest {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub user_id: i64,
    #[serde(with = "models::utils::serde::vec_i64_as_string")]
    pub role_ids: Vec<i64>,
}

#[derive(Deserialize)]
pub struct UpdateOrganizationMemberRequest {
    #[serde(with = "models::utils::serde::option_vec_i64_as_string", default)]
    pub role_ids: Option<Vec<i64>>,
    pub public_metadata: Option<serde_json::Value>,
}

// Organization role models
#[derive(Deserialize)]
pub struct CreateOrganizationRoleRequest {
    pub name: String,
    pub permissions: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateOrganizationRoleRequest {
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
}

// Workspace role models
#[derive(Deserialize)]
pub struct CreateWorkspaceRoleRequest {
    pub name: String,
    pub permissions: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateWorkspaceRoleRequest {
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
}

// Workspace member models
#[derive(Deserialize)]
pub struct AddWorkspaceMemberRequest {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub user_id: i64,
    #[serde(with = "models::utils::serde::vec_i64_as_string")]
    pub role_ids: Vec<i64>,
}

#[derive(Deserialize)]
pub struct UpdateWorkspaceMemberRequest {
    #[serde(with = "models::utils::serde::option_vec_i64_as_string", default)]
    pub role_ids: Option<Vec<i64>>,
    pub public_metadata: Option<serde_json::Value>,
}
