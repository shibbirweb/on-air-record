//! Accounts, passwords, and login sessions.
//!
//! Passwords are hashed with Argon2id at the crate's default cost, which is the OWASP recommended
//! minimum. Hashing is deliberately slow, so every entry point that hashes or verifies is synchronous and
//! the controllers call it through `spawn_blocking` rather than stalling the async runtime.
//!
//! A login session is a random 256 bit token in an HttpOnly cookie. Only its SHA-256 is stored, and it is
//! looked up on every request, so signing out, removing an account or changing a password takes effect on
//! the very next request instead of when some cached token happens to expire.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::models::{AuthMode, Role, User};
use crate::repositories::AuthRepository;
use crate::util::time::now_ms;

/// How long a login lasts. Long, because the typical client is a kitchen tablet left on the control room.
pub const SESSION_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// How often a session's last seen time is written. The status poll arrives every second, and a write per
/// poll per listener would be most of the database traffic for no benefit.
const TOUCH_INTERVAL_MS: i64 = 5 * 60 * 1000;

/// NIST SP 800-63B: at least 8 characters, no composition rules. The upper bound only stops somebody
/// posting a megabyte "password" to make the server hash it.
pub const PASSWORD_LENGTH: (usize, usize) = (8, 128);

/// Failed logins allowed from one address inside the window before it has to wait the window out.
const MAX_FAILURES: u32 = 5;
const FAILURE_WINDOW_MS: i64 = 15 * 60 * 1000;

/// What the controllers need after a successful login: who it is, and the cookie value to hand back.
pub struct SignedIn {
    pub user: User,
    pub token: String,
}

pub struct AuthService {
    repository: Arc<AuthRepository>,
    throttle: LoginThrottle,
}

impl AuthService {
    pub fn new(repository: Arc<AuthRepository>) -> Self {
        Self {
            repository,
            throttle: LoginThrottle::default(),
        }
    }

    pub fn mode(&self) -> AppResult<AuthMode> {
        self.repository.mode()
    }

    /// The account behind a session cookie, if the cookie is valid and accounts are on.
    ///
    /// Cheap: one indexed lookup. The last seen time is only written every few minutes.
    pub fn resolve(&self, token: Option<&str>) -> AppResult<Option<User>> {
        let Some(token) = token else {
            return Ok(None);
        };

        let now = now_ms();
        let token_hash = hash_token(token);
        let Some(record) = self.repository.find_session(&token_hash, now)? else {
            return Ok(None);
        };

        if now - record.last_seen_at_ms > TOUCH_INTERVAL_MS {
            if let Err(error) = self.repository.touch_session(&token_hash, now) {
                // Losing a last seen update is harmless; failing the request over it would not be.
                tracing::warn!(%error, "could not update a session's last seen time");
            }
        }

        Ok(Some(record.user))
    }

    pub fn choose_open(&self) -> AppResult<()> {
        self.repository.choose_open()?;
        tracing::info!("the install will stay open, without logins");
        Ok(())
    }

    /// Create the first admin, switch accounts on, and sign that admin in.
    pub fn set_up(&self, email: &str, password: &str) -> AppResult<SignedIn> {
        let email = validate_email(email)?;
        validate_password(password)?;

        let user = self
            .repository
            .set_up_first_admin(&email, &hash_password(password)?)?;
        tracing::info!(email = %user.email, "accounts switched on with a first admin");

        let token = self.start_session(user.id)?;
        Ok(SignedIn { user, token })
    }

    pub fn log_in(&self, client: IpAddr, email: &str, password: &str) -> AppResult<SignedIn> {
        if self.mode()? != AuthMode::Accounts {
            return Err(AppError::conflict(
                "this install has no accounts, so there is nothing to log in to",
            ));
        }

        let now = now_ms();
        self.throttle.check(client, now)?;

        let credentials = self.repository.find_credentials(email)?;
        let verified = match &credentials {
            Some(found) => verify_password(&found.password_hash, password),
            None => {
                // Spend the same time as a real check, so how long a failure takes says nothing about
                // whether the email has an account.
                verify_password(dummy_hash(), password);
                false
            }
        };

        let user = match (credentials, verified) {
            (Some(found), true) => found.user,
            _ => {
                self.throttle.record_failure(client, now);
                tracing::info!(%client, "failed login");
                return Err(AppError::unauthorized("the email or password is not right"));
            }
        };

        self.throttle.record_success(client);
        if let Err(error) = self.repository.delete_expired_sessions(now) {
            tracing::warn!(%error, "could not prune expired sessions");
        }

        let token = self.start_session(user.id)?;
        tracing::info!(email = %user.email, "logged in");
        Ok(SignedIn { user, token })
    }

    pub fn log_out(&self, token: &str) -> AppResult<()> {
        self.repository.delete_session(&hash_token(token))
    }

    /// Change your own password. Every other session of the account is signed out, which is the point of
    /// changing a password that may have leaked; the one making the change stays signed in.
    pub fn change_own_password(
        &self,
        user_id: i64,
        current_password: &str,
        new_password: &str,
        current_token: &str,
    ) -> AppResult<()> {
        let credentials = self
            .repository
            .find_credentials_by_id(user_id)?
            .ok_or_else(|| AppError::unauthorized("sign in again"))?;

        if !verify_password(&credentials.password_hash, current_password) {
            return Err(AppError::bad_request("the current password is not right"));
        }
        validate_password(new_password)?;

        self.repository
            .update_password(user_id, &hash_password(new_password)?)?;
        self.repository
            .delete_sessions_for_user(user_id, Some(&hash_token(current_token)))?;
        Ok(())
    }

    pub fn list_users(&self) -> AppResult<Vec<User>> {
        self.repository.list_users()
    }

    pub fn create_user(&self, email: &str, password: &str, role: Role) -> AppResult<User> {
        if self.mode()? != AuthMode::Accounts {
            return Err(AppError::conflict(
                "switch accounts on before adding people to them",
            ));
        }
        let email = validate_email(email)?;
        validate_password(password)?;
        let user = self
            .repository
            .create_user(&email, &hash_password(password)?, role)?;
        tracing::info!(email = %user.email, role = role.as_str(), "account created");
        Ok(user)
    }

    pub fn update_role(&self, user_id: i64, role: Role) -> AppResult<User> {
        self.repository.update_role(user_id, role)
    }

    /// An admin setting somebody else's password, typically because they forgot it. Their sessions end.
    pub fn set_password(&self, user_id: i64, new_password: &str) -> AppResult<()> {
        validate_password(new_password)?;
        self.repository
            .update_password(user_id, &hash_password(new_password)?)?;
        self.repository.delete_sessions_for_user(user_id, None)
    }

    pub fn delete_user(&self, user_id: i64) -> AppResult<()> {
        self.repository.delete_user(user_id)?;
        tracing::info!(user_id, "account removed");
        Ok(())
    }

    /// Replace an account's password with a generated one and return it. For the recovery command, run on
    /// the host by somebody who has lost access to the web interface.
    pub fn reset_password_by_email(&self, email: &str) -> AppResult<String> {
        if self.mode()? != AuthMode::Accounts {
            return Err(AppError::conflict(
                "accounts are not switched on, so there is no password to reset",
            ));
        }

        let credentials = self
            .repository
            .find_credentials(email)?
            .ok_or_else(|| AppError::not_found(format!("no account uses {}", email.trim())))?;

        let password = generate_password();
        self.set_password(credentials.user.id, &password)?;
        Ok(password)
    }

    /// Switch accounts off and forget every account. For the recovery command.
    pub fn disable_accounts(&self) -> AppResult<()> {
        self.repository.disable_accounts()
    }

    fn start_session(&self, user_id: i64) -> AppResult<String> {
        let token = generate_token();
        self.repository
            .create_session(&hash_token(&token), user_id, now_ms() + SESSION_TTL_MS)?;
        Ok(token)
    }
}

/// Failed login counts per client address.
///
/// In memory on purpose: a restart forgetting the counts is fine, and it keeps a flood of bad passwords
/// from turning into a flood of database writes.
#[derive(Default)]
struct LoginThrottle {
    attempts: Mutex<HashMap<IpAddr, Attempts>>,
}

#[derive(Clone, Copy)]
struct Attempts {
    failures: u32,
    window_started_ms: i64,
}

impl LoginThrottle {
    fn check(&self, client: IpAddr, now: i64) -> AppResult<()> {
        let Ok(attempts) = self.attempts.lock() else {
            return Ok(());
        };

        match attempts.get(&client) {
            Some(entry)
                if entry.failures >= MAX_FAILURES
                    && now - entry.window_started_ms < FAILURE_WINDOW_MS =>
            {
                let wait_minutes =
                    ((FAILURE_WINDOW_MS - (now - entry.window_started_ms)) / 60_000).max(1);
                Err(AppError::too_many_requests(format!(
                    "too many failed logins; try again in {wait_minutes} minute{}",
                    if wait_minutes == 1 { "" } else { "s" }
                )))
            }
            _ => Ok(()),
        }
    }

    fn record_failure(&self, client: IpAddr, now: i64) {
        let Ok(mut attempts) = self.attempts.lock() else {
            return;
        };

        // Keep the map from growing without bound when failures arrive from many addresses.
        if attempts.len() > 1_000 {
            attempts.retain(|_, entry| now - entry.window_started_ms < FAILURE_WINDOW_MS);
        }

        let entry = attempts.entry(client).or_insert(Attempts {
            failures: 0,
            window_started_ms: now,
        });
        if now - entry.window_started_ms >= FAILURE_WINDOW_MS {
            *entry = Attempts {
                failures: 0,
                window_started_ms: now,
            };
        }
        entry.failures += 1;
    }

    fn record_success(&self, client: IpAddr) {
        if let Ok(mut attempts) = self.attempts.lock() {
            attempts.remove(&client);
        }
    }
}

/// Normalise and sanity check an email address.
///
/// Deliberately loose. There is no mail server to prove the address works, so the email is only a login
/// name, and rejecting an unusual but valid address would be worse than accepting a typo.
pub fn validate_email(raw: &str) -> AppResult<String> {
    let email = raw.trim();
    let valid = email.len() <= 254
        && !email.chars().any(char::is_whitespace)
        && matches!(email.split_once('@'), Some((local, domain)) if !local.is_empty() && !domain.is_empty());

    if !valid {
        return Err(AppError::bad_request(
            "that does not look like an email address",
        ));
    }
    Ok(email.to_string())
}

pub fn validate_password(password: &str) -> AppResult<()> {
    let length = password.chars().count();
    if length < PASSWORD_LENGTH.0 {
        return Err(AppError::bad_request(format!(
            "the password needs at least {} characters",
            PASSWORD_LENGTH.0
        )));
    }
    if length > PASSWORD_LENGTH.1 {
        return Err(AppError::bad_request(format!(
            "the password can be at most {} characters",
            PASSWORD_LENGTH.1
        )));
    }
    Ok(())
}

fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::internal(format!("could not hash the password: {error}")))
}

fn verify_password(stored_hash: &str, password: &str) -> bool {
    match PasswordHash::new(stored_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// A real Argon2 hash of a password nobody knows, computed once, for timing equal failures.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| hash_password(&generate_token()).unwrap_or_default())
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    to_hex(&bytes)
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// A password for the recovery command to print: readable aloud, no characters that are easy to confuse.
fn generate_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 20];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    const CLIENT: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 20));

    fn service() -> AuthService {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        AuthService::new(Arc::new(AuthRepository::new(database)))
    }

    #[test]
    fn a_password_verifies_against_its_own_hash_only() {
        let hash = hash_password("correct horse").expect("hash");
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password(&hash, "correct horse"));
        assert!(!verify_password(&hash, "wrong horse"));
        assert!(!verify_password("not a hash", "correct horse"));
    }

    #[test]
    fn email_and_password_rules() {
        assert_eq!(
            validate_email("  Owner@Example.com ").expect("valid"),
            "Owner@Example.com"
        );
        for bad in ["", "owner", "@example.com", "owner@", "own er@example.com"] {
            assert!(validate_email(bad).is_err(), "{bad} should be refused");
        }

        assert!(validate_password("short").is_err());
        assert!(validate_password("eight ch").is_ok());
        assert!(validate_password(&"x".repeat(129)).is_err());
    }

    #[test]
    fn setup_signs_the_admin_in_and_login_works_afterwards() {
        let service = service();
        let signed_in = service
            .set_up("owner@example.com", "a long password")
            .expect("setup");
        assert_eq!(signed_in.user.role, Role::Admin);
        assert_eq!(
            service
                .resolve(Some(&signed_in.token))
                .expect("resolve")
                .map(|user| user.id),
            Some(signed_in.user.id)
        );

        let again = service
            .log_in(CLIENT, "OWNER@example.com", "a long password")
            .expect("login");
        assert_ne!(again.token, signed_in.token);
    }

    #[test]
    fn a_wrong_password_and_an_unknown_email_fail_the_same_way() {
        let service = service();
        service
            .set_up("owner@example.com", "a long password")
            .expect("setup");

        let wrong = service.log_in(CLIENT, "owner@example.com", "not the password");
        let unknown = service.log_in(CLIENT, "nobody@example.com", "a long password");
        assert_eq!(
            wrong.err().map(|error| error.to_string()),
            unknown.err().map(|error| error.to_string())
        );
    }

    #[test]
    fn repeated_failures_lock_the_address_out() {
        let service = service();
        service
            .set_up("owner@example.com", "a long password")
            .expect("setup");

        for _ in 0..MAX_FAILURES {
            let _ = service.log_in(CLIENT, "owner@example.com", "wrong password");
        }
        let locked = service.log_in(CLIENT, "owner@example.com", "a long password");
        assert!(matches!(locked, Err(AppError::TooManyRequests(_))));

        // Another address is not punished for this one.
        let elsewhere: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 21));
        assert!(service
            .log_in(elsewhere, "owner@example.com", "a long password")
            .is_ok());
    }

    #[test]
    fn logging_out_ends_the_session() {
        let service = service();
        let signed_in = service
            .set_up("owner@example.com", "a long password")
            .expect("setup");
        service.log_out(&signed_in.token).expect("logout");
        assert!(service
            .resolve(Some(&signed_in.token))
            .expect("resolve")
            .is_none());
    }

    #[test]
    fn changing_your_password_signs_you_out_everywhere_else() {
        let service = service();
        let here = service
            .set_up("owner@example.com", "a long password")
            .expect("setup");
        let elsewhere = service
            .log_in(CLIENT, "owner@example.com", "a long password")
            .expect("second login");

        assert!(service
            .change_own_password(here.user.id, "wrong", "a new password", &here.token)
            .is_err());
        service
            .change_own_password(
                here.user.id,
                "a long password",
                "a new password",
                &here.token,
            )
            .expect("change");

        assert!(service.resolve(Some(&here.token)).expect("here").is_some());
        assert!(service
            .resolve(Some(&elsewhere.token))
            .expect("elsewhere")
            .is_none());
        assert!(service
            .log_in(CLIENT, "owner@example.com", "a new password")
            .is_ok());
    }

    #[test]
    fn the_recovery_reset_prints_a_working_password() {
        let service = service();
        let signed_in = service
            .set_up("owner@example.com", "a long password")
            .expect("setup");

        let generated = service
            .reset_password_by_email("owner@example.com")
            .expect("reset");
        assert_eq!(generated.len(), 20);
        assert!(service
            .resolve(Some(&signed_in.token))
            .expect("old session")
            .is_none());
        assert!(service
            .log_in(CLIENT, "owner@example.com", &generated)
            .is_ok());
    }

    #[test]
    fn nothing_to_log_in_to_without_accounts() {
        let service = service();
        assert!(matches!(
            service.log_in(CLIENT, "owner@example.com", "a long password"),
            Err(AppError::Conflict(_))
        ));
    }

    #[test]
    fn tokens_are_long_random_and_hex() {
        let first = generate_token();
        let second = generate_token();
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }
}
