use std::collections::HashMap;

use serde::Deserialize;

mod entity_handlers;
mod membership_handlers;
mod query_handlers;

pub use entity_handlers::{
    create_organization, create_workspace_for_organization, delete_organization, delete_workspace,
    update_organization, update_workspace,
};
pub use membership_handlers::{
    add_organization_member, add_workspace_member, create_organization_role, create_workspace_role,
    delete_organization_role, delete_workspace_role, remove_organization_member,
    remove_workspace_member, update_organization_member, update_organization_role,
    update_workspace_member, update_workspace_role,
};
pub use query_handlers::{
    get_organization_details, get_organization_list, get_organization_members,
    get_organization_roles, get_workspace_details, get_workspace_list, get_workspace_members,
    get_workspace_roles, update_deployment_b2b_settings,
};

#[derive(Deserialize)]
pub struct OrganizationParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub organization_id: i64,
}

#[derive(Deserialize)]
pub struct OrganizationMemberParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub organization_id: i64,
    pub membership_id: i64,
}

#[derive(Deserialize)]
pub struct OrganizationRoleParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub organization_id: i64,
    pub role_id: i64,
}

#[derive(Deserialize)]
pub struct WorkspaceParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub workspace_id: i64,
}

#[derive(Deserialize)]
pub struct WorkspaceMemberParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub workspace_id: i64,
    pub membership_id: i64,
}

#[derive(Deserialize)]
pub struct OrganizationMemberQueryParams {
    pub offset: Option<i64>,
    pub limit: Option<i32>,
    pub search: Option<String>,
    pub sort_key: Option<String>,
    pub sort_order: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceRoleParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub workspace_id: i64,
    pub role_id: i64,
}

#[derive(Deserialize)]
pub struct WorkspaceMemberQueryParams {
    pub offset: Option<i64>,
    pub limit: Option<i32>,
    pub search: Option<String>,
    pub sort_key: Option<String>,
    pub sort_order: Option<String>,
}
