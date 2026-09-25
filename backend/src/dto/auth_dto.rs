//! Login, setup and account payloads.
//!
//! Password fields exist only on requests. No response ever carries a password or a hash, and the session
//! token travels only in the `Set-Cookie` header, never in a body the page's scripts could read.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::{AuthMode, Role, User};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDto {
    pub id: i64,
    pub email: String,
    pub role: Role,
    pub created_at_ms: i64,
    pub two_factor_enabled: bool,
}

impl From<User> for UserDto {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            email: user.email,
            role: user.role,
            created_at_ms: user.created_at_ms,
            two_factor_enabled: user.two_factor,
        }
    }
}

/// What the page needs to decide what to show: the first run question, the login page, or the app.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStateResponse {
    pub mode: AuthMode,
    /// The signed in account, or `null`. Always `null` without accounts.
    pub user: Option<UserDto>,
    /// True between a right password and the code, on an account with two factor sign in.
    pub pending_two_factor: bool,
}

/// Used by both setup and login, which take the same two fields.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct UserListResponse {
    pub users: Vec<UserDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub email: String,
    pub password: String,
    pub role: String,
}

impl CreateUserRequest {
    pub fn parsed_role(&self) -> AppResult<Role> {
        parse_role(&self.role)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub role: String,
}

impl UpdateUserRequest {
    pub fn parsed_role(&self) -> AppResult<Role> {
        parse_role(&self.role)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPasswordRequest {
    pub password: String,
}

/// A code from an authenticator app, or a recovery code.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeRequest {
    pub code: String,
}

/// Your own password, asked for again before changes to your second factor.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPasswordRequest {
    pub password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoFactorStatusResponse {
    pub enabled: bool,
    pub recovery_codes_left: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoFactorSetupResponse {
    /// The secret in base32, grouped in fours, for typing into an app that cannot scan.
    pub secret_key: String,
    pub otpauth_uri: String,
    /// An SVG document. The page shows it as an image, so nothing in it can run.
    pub qr_svg: String,
}

/// Recovery codes, sent once, when they are created. Only their hashes are kept.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCodesResponse {
    pub recovery_codes: Vec<String>,
}

fn parse_role(raw: &str) -> AppResult<Role> {
    Role::parse(raw.trim())
        .ok_or_else(|| AppError::bad_request("the role must be admin or listener"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_serialises_mode_and_role_in_lowercase() {
        let json = serde_json::to_value(AuthStateResponse {
            mode: AuthMode::Accounts,
            user: Some(UserDto {
                id: 3,
                email: "owner@example.com".to_string(),
                role: Role::Admin,
                created_at_ms: 1,
                two_factor_enabled: true,
            }),
            pending_two_factor: false,
        })
        .expect("serialise");
        assert_eq!(json["mode"], "accounts");
        assert_eq!(json["user"]["role"], "admin");
        assert_eq!(json["user"]["createdAtMs"], 1);
        assert!(json["user"].get("passwordHash").is_none());
        assert_eq!(json["user"]["twoFactorEnabled"], true);
        assert_eq!(json["pendingTwoFactor"], false);
        assert!(json["user"].get("totpSecret").is_none());
    }

    #[test]
    fn an_unknown_role_is_refused() {
        let request: CreateUserRequest =
            serde_json::from_str(r#"{"email":"a@b.c","password":"long enough","role":"root"}"#)
                .expect("parse");
        assert!(request.parsed_role().is_err());
    }
}
