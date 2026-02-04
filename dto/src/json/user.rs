use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub first_name: String,
    pub last_name: String,
    pub email_address: Option<String>,
    pub phone_number: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub skip_password_check: bool,
}

#[derive(Serialize, Deserialize)]
pub struct InviteUserRequest {
    pub first_name: String,
    pub last_name: String,
    pub email_address: String,
    pub expiry_days: Option<i64>,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub username: Option<String>,
    pub public_metadata: Option<Value>,
    pub private_metadata: Option<Value>,
    pub disabled: Option<bool>,
}

// Email management requests
#[derive(Serialize, Deserialize)]
pub struct AddEmailRequest {
    pub email: String,
    pub verified: Option<bool>,
    pub is_primary: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateEmailRequest {
    pub email: Option<String>,
    pub verified: Option<bool>,
    pub is_primary: Option<bool>,
}

// Phone number management requests
#[derive(Serialize, Deserialize)]
pub struct AddPhoneRequest {
    pub phone_number: String,
    pub country_code: String,
    pub verified: Option<bool>,
    pub is_primary: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct UpdatePhoneRequest {
    pub phone_number: Option<String>,
    pub country_code: Option<String>,
    pub verified: Option<bool>,
    pub is_primary: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct UpdatePasswordRequest {
    pub new_password: String,
    #[serde(default)]
    pub skip_password_check: bool,
}
