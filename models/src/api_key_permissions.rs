use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyScope {
    // User Management
    UsersRead,
    UsersWrite,
    UsersDelete,
    
    // Organization Management
    OrganizationsRead,
    OrganizationsWrite,
    OrganizationsDelete,
    OrganizationMembersRead,
    OrganizationMembersWrite,
    OrganizationRolesRead,
    OrganizationRolesWrite,
    
    // Workspace Management
    WorkspacesRead,
    WorkspacesWrite,
    WorkspacesDelete,
    WorkspaceMembersRead,
    WorkspaceMembersWrite,
    WorkspaceRolesRead,
    WorkspaceRolesWrite,
    
    // Session Management
    SessionsRead,
    SessionsWrite,
    SessionsDelete,
    
    // Auth Settings
    AuthSettingsRead,
    AuthSettingsWrite,
    
    // Social Connections
    SocialConnectionsRead,
    SocialConnectionsWrite,
    
    // JWT Templates
    JwtTemplatesRead,
    JwtTemplatesWrite,
    
    // Email/SMS Templates
    EmailTemplatesRead,
    EmailTemplatesWrite,
    SmsTemplatesRead,
    SmsTemplatesWrite,
    
    // Waitlist Management
    WaitlistRead,
    WaitlistWrite,
    
    // Deployment Settings
    DeploymentSettingsRead,
    DeploymentSettingsWrite,
    
    // Restrictions
    RestrictionsRead,
    RestrictionsWrite,
    
    // Invitations
    InvitationsRead,
    InvitationsWrite,
    InvitationsDelete,
    
    // Analytics (read-only)
    AnalyticsRead,
    
    // Notifications
    NotificationsRead,
    NotificationsWrite,
    
    // Admin Operations
    AdminAccess, // Full admin access - should be rarely granted
}

impl ApiKeyScope {
    /// Convert scope to string representation for storage
    pub fn as_str(&self) -> &'static str {
        match self {
            // User Management
            Self::UsersRead => "users:read",
            Self::UsersWrite => "users:write",
            Self::UsersDelete => "users:delete",
            
            // Organization Management
            Self::OrganizationsRead => "organizations:read",
            Self::OrganizationsWrite => "organizations:write",
            Self::OrganizationsDelete => "organizations:delete",
            Self::OrganizationMembersRead => "organization_members:read",
            Self::OrganizationMembersWrite => "organization_members:write",
            Self::OrganizationRolesRead => "organization_roles:read",
            Self::OrganizationRolesWrite => "organization_roles:write",
            
            // Workspace Management
            Self::WorkspacesRead => "workspaces:read",
            Self::WorkspacesWrite => "workspaces:write",
            Self::WorkspacesDelete => "workspaces:delete",
            Self::WorkspaceMembersRead => "workspace_members:read",
            Self::WorkspaceMembersWrite => "workspace_members:write",
            Self::WorkspaceRolesRead => "workspace_roles:read",
            Self::WorkspaceRolesWrite => "workspace_roles:write",
            
            // Session Management
            Self::SessionsRead => "sessions:read",
            Self::SessionsWrite => "sessions:write",
            Self::SessionsDelete => "sessions:delete",
            
            // Auth Settings
            Self::AuthSettingsRead => "auth_settings:read",
            Self::AuthSettingsWrite => "auth_settings:write",
            
            // Social Connections
            Self::SocialConnectionsRead => "social_connections:read",
            Self::SocialConnectionsWrite => "social_connections:write",
            
            // JWT Templates
            Self::JwtTemplatesRead => "jwt_templates:read",
            Self::JwtTemplatesWrite => "jwt_templates:write",
            
            // Email/SMS Templates
            Self::EmailTemplatesRead => "email_templates:read",
            Self::EmailTemplatesWrite => "email_templates:write",
            Self::SmsTemplatesRead => "sms_templates:read",
            Self::SmsTemplatesWrite => "sms_templates:write",
            
            // Waitlist Management
            Self::WaitlistRead => "waitlist:read",
            Self::WaitlistWrite => "waitlist:write",
            
            // Deployment Settings
            Self::DeploymentSettingsRead => "deployment_settings:read",
            Self::DeploymentSettingsWrite => "deployment_settings:write",
            
            // Restrictions
            Self::RestrictionsRead => "restrictions:read",
            Self::RestrictionsWrite => "restrictions:write",
            
            // Invitations
            Self::InvitationsRead => "invitations:read",
            Self::InvitationsWrite => "invitations:write",
            Self::InvitationsDelete => "invitations:delete",
            
            // Analytics
            Self::AnalyticsRead => "analytics:read",
            
            // Notifications
            Self::NotificationsRead => "notifications:read",
            Self::NotificationsWrite => "notifications:write",
            
            // Admin
            Self::AdminAccess => "admin:*",
        }
    }
    
    /// Parse a scope from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "users:read" => Some(Self::UsersRead),
            "users:write" => Some(Self::UsersWrite),
            "users:delete" => Some(Self::UsersDelete),
            
            "organizations:read" => Some(Self::OrganizationsRead),
            "organizations:write" => Some(Self::OrganizationsWrite),
            "organizations:delete" => Some(Self::OrganizationsDelete),
            "organization_members:read" => Some(Self::OrganizationMembersRead),
            "organization_members:write" => Some(Self::OrganizationMembersWrite),
            "organization_roles:read" => Some(Self::OrganizationRolesRead),
            "organization_roles:write" => Some(Self::OrganizationRolesWrite),
            
            "workspaces:read" => Some(Self::WorkspacesRead),
            "workspaces:write" => Some(Self::WorkspacesWrite),
            "workspaces:delete" => Some(Self::WorkspacesDelete),
            "workspace_members:read" => Some(Self::WorkspaceMembersRead),
            "workspace_members:write" => Some(Self::WorkspaceMembersWrite),
            "workspace_roles:read" => Some(Self::WorkspaceRolesRead),
            "workspace_roles:write" => Some(Self::WorkspaceRolesWrite),
            
            "sessions:read" => Some(Self::SessionsRead),
            "sessions:write" => Some(Self::SessionsWrite),
            "sessions:delete" => Some(Self::SessionsDelete),
            
            "auth_settings:read" => Some(Self::AuthSettingsRead),
            "auth_settings:write" => Some(Self::AuthSettingsWrite),
            
            "social_connections:read" => Some(Self::SocialConnectionsRead),
            "social_connections:write" => Some(Self::SocialConnectionsWrite),
            
            "jwt_templates:read" => Some(Self::JwtTemplatesRead),
            "jwt_templates:write" => Some(Self::JwtTemplatesWrite),
            
            "email_templates:read" => Some(Self::EmailTemplatesRead),
            "email_templates:write" => Some(Self::EmailTemplatesWrite),
            "sms_templates:read" => Some(Self::SmsTemplatesRead),
            "sms_templates:write" => Some(Self::SmsTemplatesWrite),
            
            "waitlist:read" => Some(Self::WaitlistRead),
            "waitlist:write" => Some(Self::WaitlistWrite),
            
            "deployment_settings:read" => Some(Self::DeploymentSettingsRead),
            "deployment_settings:write" => Some(Self::DeploymentSettingsWrite),
            
            "restrictions:read" => Some(Self::RestrictionsRead),
            "restrictions:write" => Some(Self::RestrictionsWrite),
            
            "invitations:read" => Some(Self::InvitationsRead),
            "invitations:write" => Some(Self::InvitationsWrite),
            "invitations:delete" => Some(Self::InvitationsDelete),
            
            "analytics:read" => Some(Self::AnalyticsRead),
            
            "notifications:read" => Some(Self::NotificationsRead),
            "notifications:write" => Some(Self::NotificationsWrite),
            
            "admin:*" => Some(Self::AdminAccess),
            
            _ => None,
        }
    }
    
    /// Get all available scopes
    pub fn all() -> Vec<Self> {
        vec![
            Self::UsersRead,
            Self::UsersWrite,
            Self::UsersDelete,
            Self::OrganizationsRead,
            Self::OrganizationsWrite,
            Self::OrganizationsDelete,
            Self::OrganizationMembersRead,
            Self::OrganizationMembersWrite,
            Self::OrganizationRolesRead,
            Self::OrganizationRolesWrite,
            Self::WorkspacesRead,
            Self::WorkspacesWrite,
            Self::WorkspacesDelete,
            Self::WorkspaceMembersRead,
            Self::WorkspaceMembersWrite,
            Self::WorkspaceRolesRead,
            Self::WorkspaceRolesWrite,
            Self::SessionsRead,
            Self::SessionsWrite,
            Self::SessionsDelete,
            Self::AuthSettingsRead,
            Self::AuthSettingsWrite,
            Self::SocialConnectionsRead,
            Self::SocialConnectionsWrite,
            Self::JwtTemplatesRead,
            Self::JwtTemplatesWrite,
            Self::EmailTemplatesRead,
            Self::EmailTemplatesWrite,
            Self::SmsTemplatesRead,
            Self::SmsTemplatesWrite,
            Self::WaitlistRead,
            Self::WaitlistWrite,
            Self::DeploymentSettingsRead,
            Self::DeploymentSettingsWrite,
            Self::RestrictionsRead,
            Self::RestrictionsWrite,
            Self::InvitationsRead,
            Self::InvitationsWrite,
            Self::InvitationsDelete,
            Self::AnalyticsRead,
            Self::NotificationsRead,
            Self::NotificationsWrite,
            Self::AdminAccess,
        ]
    }
    
    /// Get default scopes for a new API key (read-only access)
    pub fn default_scopes() -> Vec<Self> {
        vec![
            Self::UsersRead,
            Self::OrganizationsRead,
            Self::WorkspacesRead,
            Self::SessionsRead,
            Self::AuthSettingsRead,
            Self::AnalyticsRead,
        ]
    }
    
    /// Get scopes for a read-only API key
    pub fn readonly_scopes() -> Vec<Self> {
        vec![
            Self::UsersRead,
            Self::OrganizationsRead,
            Self::OrganizationMembersRead,
            Self::OrganizationRolesRead,
            Self::WorkspacesRead,
            Self::WorkspaceMembersRead,
            Self::WorkspaceRolesRead,
            Self::SessionsRead,
            Self::AuthSettingsRead,
            Self::SocialConnectionsRead,
            Self::JwtTemplatesRead,
            Self::EmailTemplatesRead,
            Self::SmsTemplatesRead,
            Self::WaitlistRead,
            Self::DeploymentSettingsRead,
            Self::RestrictionsRead,
            Self::InvitationsRead,
            Self::AnalyticsRead,
            Self::NotificationsRead,
        ]
    }
    
    /// Get scopes for a read-write API key (no delete permissions)
    pub fn readwrite_scopes() -> Vec<Self> {
        vec![
            Self::UsersRead,
            Self::UsersWrite,
            Self::OrganizationsRead,
            Self::OrganizationsWrite,
            Self::OrganizationMembersRead,
            Self::OrganizationMembersWrite,
            Self::OrganizationRolesRead,
            Self::OrganizationRolesWrite,
            Self::WorkspacesRead,
            Self::WorkspacesWrite,
            Self::WorkspaceMembersRead,
            Self::WorkspaceMembersWrite,
            Self::WorkspaceRolesRead,
            Self::WorkspaceRolesWrite,
            Self::SessionsRead,
            Self::SessionsWrite,
            Self::AuthSettingsRead,
            Self::AuthSettingsWrite,
            Self::SocialConnectionsRead,
            Self::SocialConnectionsWrite,
            Self::JwtTemplatesRead,
            Self::JwtTemplatesWrite,
            Self::EmailTemplatesRead,
            Self::EmailTemplatesWrite,
            Self::SmsTemplatesRead,
            Self::SmsTemplatesWrite,
            Self::WaitlistRead,
            Self::WaitlistWrite,
            Self::DeploymentSettingsRead,
            Self::DeploymentSettingsWrite,
            Self::RestrictionsRead,
            Self::RestrictionsWrite,
            Self::InvitationsRead,
            Self::InvitationsWrite,
            Self::AnalyticsRead,
            Self::NotificationsRead,
            Self::NotificationsWrite,
        ]
    }
}

impl fmt::Display for ApiKeyScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Helper functions for validating and converting scopes
pub struct ApiKeyScopeHelper;

impl ApiKeyScopeHelper {
    /// Validate a list of scope strings
    pub fn validate_scopes(scopes: &[String]) -> Result<Vec<ApiKeyScope>, Vec<String>> {
        let mut valid_scopes = Vec::new();
        let mut invalid_scopes = Vec::new();
        
        for scope_str in scopes {
            if let Some(scope) = ApiKeyScope::from_str(scope_str) {
                valid_scopes.push(scope);
            } else {
                invalid_scopes.push(scope_str.clone());
            }
        }
        
        if !invalid_scopes.is_empty() {
            Err(invalid_scopes)
        } else {
            Ok(valid_scopes)
        }
    }
    
    /// Convert scopes to string representation for storage
    pub fn scopes_to_strings(scopes: &[ApiKeyScope]) -> Vec<String> {
        scopes.iter().map(|s| s.as_str().to_string()).collect()
    }
    
    /// Parse scopes from strings
    pub fn strings_to_scopes(scope_strings: &[String]) -> Vec<ApiKeyScope> {
        scope_strings
            .iter()
            .filter_map(|s| ApiKeyScope::from_str(s))
            .collect()
    }
    
    /// Check if a set of scopes includes a specific scope
    pub fn has_scope(scopes: &[String], required: ApiKeyScope) -> bool {
        scopes.contains(&required.as_str().to_string()) || 
        scopes.contains(&ApiKeyScope::AdminAccess.as_str().to_string())
    }
    
    /// Check if a set of scopes includes any of the required scopes
    pub fn has_any_scope(scopes: &[String], required: &[ApiKeyScope]) -> bool {
        if scopes.contains(&ApiKeyScope::AdminAccess.as_str().to_string()) {
            return true;
        }
        
        required.iter().any(|req| scopes.contains(&req.as_str().to_string()))
    }
    
    /// Check if a set of scopes includes all of the required scopes
    pub fn has_all_scopes(scopes: &[String], required: &[ApiKeyScope]) -> bool {
        if scopes.contains(&ApiKeyScope::AdminAccess.as_str().to_string()) {
            return true;
        }
        
        required.iter().all(|req| scopes.contains(&req.as_str().to_string()))
    }
}