use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::Deserialize;

use super::SortOrder;

#[derive(PartialEq, Deserialize)]
pub enum ActiveUserListSortKey {
    CreatedAt,
    Username,
    Email,
    PhoneNumber,
}

impl Default for ActiveUserListSortKey {
    fn default() -> Self {
        Self::CreatedAt
    }
}

impl FromStr for ActiveUserListSortKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "created_at" => Ok(ActiveUserListSortKey::CreatedAt),
            "username" => Ok(ActiveUserListSortKey::Username),
            "email" => Ok(ActiveUserListSortKey::Email),
            "phone_number" => Ok(ActiveUserListSortKey::PhoneNumber),
            _ => Err("Invalid sort key".to_string()),
        }
    }
}

#[derive(PartialEq, Deserialize)]
pub enum InvitationsWaitlistSortKey {
    CreatedAt,
    Email,
}

impl Default for InvitationsWaitlistSortKey {
    fn default() -> Self {
        Self::CreatedAt
    }
}

impl FromStr for InvitationsWaitlistSortKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "created_at" => Ok(InvitationsWaitlistSortKey::CreatedAt),
            "email" => Ok(InvitationsWaitlistSortKey::Email),
            _ => Err("Invalid sort key".to_string()),
        }
    }
}

impl Display for ActiveUserListSortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreatedAt => write!(f, "created_at"),
            Self::Username => write!(f, "username"),
            Self::Email => write!(f, "email"),
            Self::PhoneNumber => write!(f, "phone_number"),
        }
    }
}

impl Display for InvitationsWaitlistSortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreatedAt => write!(f, "created_at"),
            Self::Email => write!(f, "email"),
        }
    }
}

#[derive(Deserialize)]
pub struct ActiveUserListQueryParams {
    pub offset: Option<i64>,
    #[serde(
        default,
        deserialize_with = "crate::utils::serde::enum_from_str::from_str_option"
    )]
    pub sort_key: Option<ActiveUserListSortKey>,
    #[serde(
        default,
        deserialize_with = "crate::utils::serde::enum_from_str::from_str_option"
    )]
    pub sort_order: Option<SortOrder>,
    pub limit: Option<usize>,
}

impl Default for ActiveUserListQueryParams {
    fn default() -> Self {
        Self {
            offset: Some(0),
            sort_key: Some(ActiveUserListSortKey::CreatedAt),
            sort_order: Some(SortOrder::Desc),
            limit: Some(10),
        }
    }
}

#[derive(Deserialize)]
pub struct InvitationsWaitlistQueryParams {
    pub offset: Option<i64>,
    #[serde(
        default,
        deserialize_with = "crate::utils::serde::enum_from_str::from_str_option"
    )]
    pub sort_key: Option<InvitationsWaitlistSortKey>,
    #[serde(
        default,
        deserialize_with = "crate::utils::serde::enum_from_str::from_str_option"
    )]
    pub sort_order: Option<SortOrder>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct OrganizationListQueryParams {
    pub offset: Option<i64>,
    pub sort_key: Option<String>,
    pub sort_order: Option<String>,
    pub limit: Option<i32>,
}

impl Default for OrganizationListQueryParams {
    fn default() -> Self {
        Self {
            offset: Some(0),
            sort_key: Some("created_at".to_string()),
            sort_order: Some("desc".to_string()),
            limit: Some(10),
        }
    }
}

// AI-related query parameters
#[derive(Deserialize)]
pub struct GetAgentsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub search: Option<String>,
}

#[derive(Deserialize)]
pub struct GetToolsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub search: Option<String>,
}

#[derive(Deserialize)]
pub struct GetWorkflowsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub search: Option<String>,
}

#[derive(Deserialize)]
pub struct GetKnowledgeBasesQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub search: Option<String>,
}
