use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct UserParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub user_id: i64,
}

#[derive(Deserialize)]
pub struct UserEmailParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub user_id: i64,
    pub email_id: i64,
}

#[derive(Deserialize)]
pub struct UserPhoneParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub user_id: i64,
    pub phone_id: i64,
}

#[derive(Deserialize)]
pub struct UserSocialParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub user_id: i64,
    pub connection_id: i64,
}

#[derive(Deserialize)]
pub struct WaitlistUserParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub waitlist_user_id: i64,
}

#[derive(Deserialize)]
pub struct InvitationParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub invitation_id: i64,
}
