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
    pub webhook_failure_notification_template: EmailTemplate,
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
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "{{inviter_name}} invited you to join {{organization_name}} on {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">You've been invited to {{organization_name}}</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">{{inviter_name}} has invited you to join <span style="font-weight:400">{{organization_name}}</span> on {{app.name}}.</p>
<div style="text-align:center;">
<a href="{{action_url}}" style="display:inline-block;background-color:#000000;color:#ffffff;padding:14px 28px;border-radius:6px;text-decoration:none;font-size:15px;font-weight:400;margin-bottom:24px;">Accept Invitation</a>
</div>
<p style="margin:0 0 16px 0;font-size:14px;line-height:24px;color:#6b7280;">This invitation expires in {{invitation.expires_in_days}} days.</p>
<p style="margin:0;font-size:14px;line-height:24px;color:#6b7280;">Or copy: <a href="{{action_url}}" style="color:#000000;text-decoration:none;word-break:break-all;">{{action_url}}</a></p>
<p style="margin-top:32px;font-size:12px;color:#9ca3af;">If you weren't expecting this invitation, you can ignore this email.</p>"#.to_string(),
            },
            verification_code_template: EmailTemplate {
                template_name: "Verification Code".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "{{code.value}} is your verification code for {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">Verify your identity</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">Use the verification code below to complete your sign-in to <span style="font-weight:400">{{app.name}}</span>.</p>
<div style="font-family:monospace;font-size:24px;font-weight:400;letter-spacing:4px;color:#111827;margin-bottom:24px;text-align:center;">{{code.value}}</div>
<p style="margin:0 0 24px 0;font-size:14px;line-height:24px;color:#6b7280;">This code expires in {{code.expires_in_minutes}} minutes.</p>
{{#if device.info}}<p style="margin:0;font-size:12px;color:#9ca3af;">Requested from: {{device.info}}</p>{{/if}}"#.to_string(),
            },
            reset_password_code_template: EmailTemplate {
                template_name: "Reset Password Code".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Reset your password for {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">Reset your password</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">Use the following code to reset your password for <span style="font-weight:400">{{app.name}}</span>.</p>
<div style="font-family:monospace;font-size:24px;font-weight:400;letter-spacing:4px;color:#111827;margin-bottom:24px;text-align:center;">{{code.value}}</div>
<p style="margin:0 0 24px 0;font-size:14px;line-height:24px;color:#6b7280;">This code expires in {{code.expires_in_minutes}} minutes.</p>
<p style="margin:0;font-size:12px;color:#9ca3af;">If you didn't request a password reset, you can safely ignore this email.</p>"#.to_string(),
            },
            primary_email_change_template: EmailTemplate {
                template_name: "Email Address Changed".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Your email address was changed on {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">Email Changed</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">We noticed your primary email address on <span style="font-weight:400">{{app.name}}</span> was just updated.</p>
<p style="margin:0;font-size:14px;line-height:24px;color:#6b7280;">If you didn't make this change, please contact support immediately.</p>"#.to_string(),
            },
            password_change_template: EmailTemplate {
                template_name: "Password Changed".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Your password was changed on {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">Password Changed</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">The password for your <span style="font-weight:400">{{app.name}}</span> account has been successfully updated.</p>
<p style="margin:0;font-size:14px;line-height:24px;color:#6b7280;">If you didn't make this change, please reset your password immediately.</p>"#.to_string(),
            },
            password_remove_template: EmailTemplate {
                template_name: "Password Removed".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Your password was removed from your {{app.name}} account".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">Password Removed</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">The password for your <span style="font-weight:400">{{app.name}}</span> account has been removed.</p>
<p style="margin:0;font-size:14px;line-height:24px;color:#6b7280;">If you didn't make this change, please contact support immediately.</p>"#.to_string(),
            },
            sign_in_from_new_device_template: EmailTemplate {
                template_name: "New Device Sign In".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "New sign-in to your {{app.name}} account".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">New sign-in detected</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">We detected a new sign-in to your <span style="font-weight:400">{{app.name}}</span> account.</p>
{{#if device.info}}<p style="margin:0;font-size:14px;line-height:24px;color:#4b5563;"><span style="font-weight:400">Device:</span> {{device.info}}</p>{{/if}}
<p style="margin:24px 0 0 0;font-size:14px;line-height:24px;color:#6b7280;">If this wasn't you, please secure your account immediately by resetting your password.</p>"#.to_string(),
            },
            magic_link_template: EmailTemplate {
                template_name: "Magic Link Sign In".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Sign in to {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">Welcome back!</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">Sign in to your <span style="font-weight:400">{{app.name}}</span> account.</p>
<div style="text-align:center;">
<a href="{{action_url}}" style="display:inline-block;background-color:#000000;color:#ffffff;padding:14px 28px;border-radius:6px;text-decoration:none;font-size:15px;font-weight:400;margin-bottom:24px;">Sign In</a>
</div>
<p style="margin:0 0 16px 0;font-size:14px;line-height:24px;color:#6b7280;">Or copy: <a href="{{action_url}}" style="color:#000000;text-decoration:none;word-break:break-all;">{{action_url}}</a></p>
<p style="margin-top:32px;font-size:12px;color:#9ca3af;">If you didn't request this link, ignore this email.</p>"#.to_string(),
            },
            waitlist_signup_template: EmailTemplate {
                template_name: "Added to Waitlist".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "You're on the waitlist for {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">You're on the Waitlist</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">Thank you for your interest in <span style="font-weight:400">{{app.name}}</span>. You've been added to our waitlist.</p>
<p style="margin:0;font-size:14px;line-height:24px;color:#6b7280;">We'll notify you as soon as a spot becomes available.</p>"#.to_string(),
            },
            waitlist_invite_template: EmailTemplate {
                template_name: "App Invitation".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Your invitation for {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">Your Invitation</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">You've been invited to join <span style="font-weight:400">{{app.name}}</span>.</p>
<div style="text-align:center;">
<a href="{{action_url}}" style="display:inline-block;background-color:#000000;color:#ffffff;padding:14px 28px;border-radius:6px;text-decoration:none;font-size:15px;font-weight:400;margin-bottom:24px;">Get Started</a>
</div>
<p style="margin:0 0 16px 0;font-size:14px;line-height:24px;color:#6b7280;">This invitation expires in {{invitation.expires_in_days}} days.</p>
<p style="margin:0;font-size:14px;line-height:24px;color:#6b7280;">Or copy: <a href="{{action_url}}" style="color:#000000;text-decoration:none;word-break:break-all;">{{action_url}}</a></p>"#.to_string(),
            },
            workspace_invite_template: EmailTemplate {
                template_name: "Workspace Invitation".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "You've been invited to join {{app.name}}".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">You're invited!</h1>
<p style="margin:0 0 32px 0;font-size:16px;line-height:26px;color:#374151;">{{#if inviter_name}}<span style="font-weight:400">{{inviter_name}}</span> has invited you to join them on <span style="font-weight:400">{{app.name}}</span>.{{else}}You've been invited to join <span style="font-weight:400">{{app.name}}</span>.{{/if}}</p>
<div style="text-align:center;">
<a href="{{action_url}}" style="display:inline-block;background-color:#000000;color:#ffffff;padding:14px 28px;border-radius:6px;text-decoration:none;font-size:15px;font-weight:400;margin-bottom:24px;">Accept Invitation</a>
</div>
<p style="margin:0 0 16px 0;font-size:14px;line-height:24px;color:#6b7280;">This invitation expires in {{invitation.expires_in_days}} days.</p>
<p style="margin:0;font-size:14px;line-height:24px;color:#6b7280;">Or copy: <a href="{{action_url}}" style="color:#000000;text-decoration:none;word-break:break-all;">{{action_url}}</a></p>
<p style="margin-top:32px;font-size:12px;color:#9ca3af;">If you weren't expecting this invitation, you can safely ignore this email.</p>"#.to_string(),
            },
            webhook_failure_notification_template: EmailTemplate {
                template_name: "Webhook Failure Notification".to_string(),
                template_from: "notification".to_string(),
                template_reply_to: "".to_string(),
                template_subject: "Webhook endpoint disabled".to_string(),
                template_data: r#"<div style="margin-bottom:48px;text-align:center;">{{image app.logo app.name}}</div>
<h1 style="margin:0 0 16px 0;font-size:24px;font-weight:400;letter-spacing:-0.5px;color:#111827;">Webhook endpoint disabled</h1>
<p style="margin:0 0 24px 0;font-size:16px;line-height:26px;color:#374151;">This endpoint was automatically disabled after repeated delivery failures.</p>
<p style="margin:0;font-size:16px;line-height:26px;color:#374151;">Endpoint: <span style="font-weight:400">{{endpoint.url}}</span></p>"#.to_string(),
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
