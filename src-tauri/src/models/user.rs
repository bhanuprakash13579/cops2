use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub id: i64,
    pub user_name: String,
    pub user_desig: Option<String>,
    pub user_id: String,
    pub user_pwd: String,
    pub user_role: String,
    pub user_status: Option<String>,
    pub created_by: Option<String>,
    pub created_on: Option<String>,
    pub closed_on: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserOut {
    pub id: i64,
    pub user_name: String,
    pub user_desig: Option<String>,
    pub user_id: String,
    pub user_role: String,
    pub user_status: Option<String>,
    pub created_on: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub user_id: String,
    pub password: String,
    /// Optional: "sdo" | "adjudication" | "query" | "apis"
    /// When provided, validates that the user's role is permitted for this module.
    pub module_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub token_type: String,
    pub user_name: String,
    pub user_id: String,
    pub user_role: String,
    pub user_desig: Option<String>,
    pub user_status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub user_name: String,
    pub user_desig: Option<String>,
    pub user_id: String,
    pub password: String,
    pub user_role: String,
    /// Only the hidden system-admin path reads this — it designates the user
    /// admin. The in-app `create_user` ignores it, so a user admin cannot mint
    /// another user admin; only the system admin can.
    #[serde(default)]
    pub is_user_admin: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}
