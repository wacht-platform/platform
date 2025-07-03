use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct EmailTemplate {
    pub template_name: String,
    pub template_data: String,
    pub template_from: String,
    pub template_reply_to: String,
    pub template_subject: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentEmailTemplate {
    pub id: i64,
    pub deployment_id: i64,
    pub organization_invite_template: EmailTemplate,
    pub verification_code_template: EmailTemplate,
    pub reset_password_code_template: EmailTemplate,
    pub primary_email_change_template: EmailTemplate,
    pub password_change_template: EmailTemplate,
    pub password_remove_template: EmailTemplate,
    pub sign_in_from_new_device_template: EmailTemplate,
    pub magic_link_template: EmailTemplate,
    pub waitlist_signup_template: EmailTemplate,
    pub waitlist_invite_template: EmailTemplate,
    pub workspace_invite_template: EmailTemplate,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Default for DeploymentEmailTemplate {
    fn default() -> Self {
        Self {
            id: 0,
            deployment_id: 0,
            organization_invite_template: EmailTemplate {
                template_name: "Organization Invitation".to_string(),
                template_from: "invitations".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Invitation to join {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">You're Invited to Join {{app_name}}</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    You have been invited to join {{app_name}}. Click the button below to accept the invitation.
                </p>
                 <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    This invitation will expire in {{invitation.expires_in_days}} days.
                </p>
                <div style="text-align: center; margin: 32px 0 0 0;"><a href="{{action_url}}" style="display: inline-block; padding: 16px 32px; font-size: 16px; color: #ffffff; background-color: #6c47ff; border-radius: 8px; text-decoration: none; font-weight: 500; line-height: 1; box-shadow: 0px 2px 4px rgba(0, 0, 0, 0.1);" class="cl-branded-button">Accept Invitation</a></div>
                <p style="margin: 24px 0 0 0; text-align: center; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">If you're having trouble with the above button, <a href="{{action_url}}" style="text-decoration: none; color: #6c47ff;" class="cl-branded-link">click here</a>.</p>
            </div>
        </div>"#.to_string(),
            },
            verification_code_template: EmailTemplate {
                template_name: "Verification Code".to_string(),
                template_from: "verification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Your verification code for {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">Verification Code</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    Enter the following verification code when prompted:
                </p>
                <p style="margin-top: 16px; text-align: center; font-size: 24px; color: #000000; font-weight: bold; line-height: 32px;">
                    {{code}}
                </p>
                <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    This code will expire in {{code.expires_in_minutes}} minutes. If you didn't request this code, you can safely ignore this email.
                </p>
            </div>
        </div>"#.to_string(),
            },
            reset_password_code_template: EmailTemplate {
                template_name: "Reset Password Code".to_string(),
                template_from: "security".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Reset your password for {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">Reset Your Password</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    Click the button below to reset your password.
                </p>
                 <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    If you didn't request a password reset, you can safely ignore this email. This link will expire in {{code.expires_in_minutes}} minutes.
                </p>
                <div style="text-align: center; margin: 32px 0 0 0;"><a href="{{action_url}}" style="display: inline-block; padding: 16px 32px; font-size: 16px; color: #ffffff; background-color: #6c47ff; border-radius: 8px; text-decoration: none; font-weight: 500; line-height: 1; box-shadow: 0px 2px 4px rgba(0, 0, 0, 0.1);" class="cl-branded-button">Reset Password</a></div>
                <p style="margin: 24px 0 0 0; text-align: center; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">If you're having trouble with the above button, <a href="{{action_url}}" style="text-decoration: none; color: #6c47ff;" class="cl-branded-link">click here</a>.</p>
            </div>
        </div>"#.to_string(),
            },
            primary_email_change_template: EmailTemplate {
                template_name: "Email Address Changed".to_string(),
                template_from: "security".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Your primary email address was changed on {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">Primary Email Address Updated</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    This email confirms that the primary email address associated with your {{app_name}} account was recently changed.
                </p>
                 <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    If you did not make this change, please contact our support team immediately.
                </p>
            </div>
        </div>"#.to_string(),
            },
            password_change_template: EmailTemplate {
                template_name: "Password Changed".to_string(),
                template_from: "security".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Your password was changed on {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">Password Successfully Changed</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    This email confirms that the password for your {{app_name}} account was successfully changed.
                </p>
                 <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    If you did not make this change, please reset your password immediately and contact our support team.
                </p>
            </div>
        </div>"#.to_string(),
            },
            password_remove_template: EmailTemplate {
                template_name: "Password Removed".to_string(),
                template_from: "security".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Your password was removed from your {{app_name}} account".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">Password Removed</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    This email confirms that the password associated with your {{app_name}} account has been removed. You may now need to use alternative sign-in methods (like magic links or social providers) if enabled.
                </p>
                 <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    If you did not request this change, please contact our support team immediately.
                </p>
            </div>
        </div>"#.to_string(),
            },
            sign_in_from_new_device_template: EmailTemplate {
                template_name: "New Device Sign In".to_string(),
                template_from: "security".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Sign in from a new device detected on {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">New Device Sign-In Detected</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    We detected a sign-in to your {{app_name}} account from a new device or location.
                </p>
                {{#if device_info}}
                <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    Device details: {{device_info}}
                </p>
                {{/if}}
                <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    If this was you, you can safely ignore this email. If you don't recognize this activity, please secure your account immediately by resetting your password and reviewing your security settings.
                </p>
            </div>
        </div>"#.to_string(),
            },
            magic_link_template: EmailTemplate {
                template_name: "Magic Link Sign In".to_string(),
                template_from: "authentication".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Sign in to {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">Sign In to {{app_name}}</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    Click the button below to sign in to your account.
                </p>
                 <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    This link will expire in {{link.expires_in_minutes}} minutes. If you didn't request this link, you can safely ignore this email.
                </p>
                <div style="text-align: center; margin: 32px 0 0 0;"><a href="{{action_url}}" style="display: inline-block; padding: 16px 32px; font-size: 16px; color: #ffffff; background-color: #6c47ff; border-radius: 8px; text-decoration: none; font-weight: 500; line-height: 1; box-shadow: 0px 2px 4px rgba(0, 0, 0, 0.1);" class="cl-branded-button">Sign In</a></div>
                <p style="margin: 24px 0 0 0; text-align: center; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">If you're having trouble with the above button, <a href="{{action_url}}" style="text-decoration: none; color: #6c47ff;" class="cl-branded-link">click here</a>.</p>
            </div>
        </div>"#.to_string(),
            },
            waitlist_signup_template: EmailTemplate {
                template_name: "Added to Waitlist".to_string(),
                template_from: "notifications".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "You're on the waitlist for {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">You're on the Waitlist!</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    Thanks for your interest in {{app_name}}! You've been successfully added to our waitlist.
                </p>
                 <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    We'll notify you as soon as a spot becomes available.
                </p>
            </div>
        </div>"#.to_string(),
            },
            waitlist_invite_template: EmailTemplate {
                template_name: "Waitlist Invitation".to_string(),
                template_from: "invitations".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "You're invited to join {{app_name}} from the waitlist!".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">You're Invited!</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    Good news! You've been invited to join {{app_name}} from the waitlist. Click the button below to accept your invitation and get started.
                </p>
                 <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    This invitation will expire in {{invitation.expires_in_days}} days.
                </p>
                <div style="text-align: center; margin: 32px 0 0 0;"><a href="{{action_url}}" style="display: inline-block; padding: 16px 32px; font-size: 16px; color: #ffffff; background-color: #6c47ff; border-radius: 8px; text-decoration: none; font-weight: 500; line-height: 1; box-shadow: 0px 2px 4px rgba(0, 0, 0, 0.1);" class="cl-branded-button">Accept Invitation</a></div>
                <p style="margin: 24px 0 0 0; text-align: center; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">If you're having trouble with the above button, <a href="{{action_url}}" style="text-decoration: none; color: #6c47ff;" class="cl-branded-link">click here</a>.</p>
            </div>
        </div>"#.to_string(),
            },
            workspace_invite_template: EmailTemplate {
                template_name: "Invitation".to_string(), // Assuming this corresponds to "workspace-invitation"
                template_from: "invitations".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Invitation to join {{app_name}}".to_string(),
                template_data: r#"
        <div style="padding: 48px 32px; background-color: #f9fafb;">
            <div style="text-align: center; margin-bottom: 32px; font-size: 32px; line-height: 1.4;">
                {{image app_logo}}
            </div>
            <div style="background-color: #ffffff; border-radius: 0px; padding: 32px 32px 48px 32px; margin: 0; text-align: left; box-shadow: 0px 1px 3px rgba(0, 0, 0, 0.1);">
                <h1 style="color: #000000; text-align: left; margin-top: 0; margin-bottom: 16px; font-weight: 500; font-size: 28px; line-height: 36px;">Your invitation</h1>
                <p style="text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    {{#if inviter_name}}{{escapeURIs inviter_name}} has invited you to join them on {{app_name}}.{{else}}You are invited to join {{app_name}}.{{/if}}
                </p>
                <p style="margin-top: 16px; text-align: left; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">
                    This invitation will expire in {{invitation.expires_in_days}} days.
                </p>
                <div style="text-align: center; margin: 32px 0 0 0;"><a href="{{action_url}}" style="display: inline-block; padding: 16px 32px; font-size: 16px; color: #ffffff; background-color: #6c47ff; border-radius: 8px; text-decoration: none; font-weight: 500; line-height: 1; box-shadow: 0px 2px 4px rgba(0, 0, 0, 0.1);" class="cl-branded-button">Accept invitation</a></div>
                <p style="margin: 24px 0 0 0; text-align: center; font-size: 16px; color: #000000; font-weight: normal; line-height: 26px;">If you're having trouble with the above button, <a href="{{action_url}}" style="text-decoration: none; color: #6c47ff;" class="cl-branded-link">click here</a>.</p>
            </div>
        </div>"#.to_string(),
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
