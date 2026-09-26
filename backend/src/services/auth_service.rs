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
use qrcode::render::svg;
use qrcode::QrCode;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::models::{AuthMode, Role, User};
use crate::repositories::AuthRepository;
use crate::services::totp;
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

/// How long somebody has, after a right password, to type the code from their authenticator app.
const CHALLENGE_TTL_MS: i64 = 5 * 60 * 1000;

/// Wrong codes allowed against one pending sign in before the password has to be typed again.
const MAX_CODE_ATTEMPTS: u32 = 5;

/// Recovery codes handed out when two factor sign in is switched on, each usable once.
pub const RECOVERY_CODE_COUNT: usize = 10;

/// The name an authenticator app lists the account under.
const ISSUER: &str = "On Air Record";

/// What the controllers need after a successful login: who it is, and the cookie value to hand back.
pub struct SignedIn {
    pub user: User,
    pub token: String,
}

/// What a right password leads to.
pub enum LoginOutcome {
    /// No second factor on the account, so a session exists already.
    SignedIn(SignedIn),
    /// The account has two factor sign in. No session exists yet; the challenge token, kept in its own
    /// short lived cookie, is what the code is checked against.
    SecondFactorRequired { challenge: String },
}

impl LoginOutcome {
    /// The session, when the password alone was enough.
    pub fn signed_in(self) -> Option<SignedIn> {
        match self {
            Self::SignedIn(signed_in) => Some(signed_in),
            Self::SecondFactorRequired { .. } => None,
        }
    }
}

/// What the page shows while somebody sets up their authenticator app.
pub struct TwoFactorSetup {
    /// The secret in base32, in groups of four, for typing in when the QR code cannot be scanned.
    pub secret_key: String,
    pub otpauth_uri: String,
    /// The `otpauth` URI as a QR code, an SVG document.
    pub qr_svg: String,
}

/// A sign in that got the password right and is waiting for the code.
///
/// In memory, like the throttle: a restart only means typing the password again.
struct Challenge {
    user_id: i64,
    expires_at_ms: i64,
    attempts: u32,
}

pub struct AuthService {
    repository: Arc<AuthRepository>,
    throttle: LoginThrottle,
    challenges: Mutex<HashMap<Vec<u8>, Challenge>>,
}

impl AuthService {
    pub fn new(repository: Arc<AuthRepository>) -> Self {
        Self {
            repository,
            throttle: LoginThrottle::default(),
            challenges: Mutex::new(HashMap::new()),
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

    /// Check an email and password. With two factor sign in on the account, the right password only
    /// earns a challenge, and [`Self::verify_second_factor`] turns it into a session.
    pub fn log_in(&self, client: IpAddr, email: &str, password: &str) -> AppResult<LoginOutcome> {
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

        if user.two_factor {
            // The password was right, but the throttle keeps counting until the code is too, so the
            // password cannot be used to reset the count for somebody guessing codes.
            let challenge = self.start_challenge(user.id, now);
            tracing::info!(email = %user.email, "password accepted, waiting for the code");
            return Ok(LoginOutcome::SecondFactorRequired { challenge });
        }

        self.throttle.record_success(client);
        if let Err(error) = self.repository.delete_expired_sessions(now) {
            tracing::warn!(%error, "could not prune expired sessions");
        }

        let token = self.start_session(user.id)?;
        tracing::info!(email = %user.email, "logged in");
        Ok(LoginOutcome::SignedIn(SignedIn { user, token }))
    }

    /// Whether a challenge cookie still has a sign in waiting on it, so a reload keeps showing the code
    /// step rather than dropping back to the password.
    pub fn has_pending_challenge(&self, challenge: Option<&str>) -> bool {
        let Some(challenge) = challenge else {
            return false;
        };
        let Ok(challenges) = self.challenges.lock() else {
            return false;
        };
        challenges
            .get(&hash_token(challenge))
            .is_some_and(|pending| pending.expires_at_ms > now_ms())
    }

    /// Finish a sign in with a code from the authenticator app, or with a recovery code.
    pub fn verify_second_factor(
        &self,
        client: IpAddr,
        challenge: &str,
        code: &str,
    ) -> AppResult<SignedIn> {
        let now = now_ms();
        self.throttle.check(client, now)?;

        let challenge_hash = hash_token(challenge);
        let user_id = {
            let mut challenges = self
                .challenges
                .lock()
                .map_err(|_| AppError::internal("the sign in list lock was poisoned"))?;
            match challenges.get(&challenge_hash) {
                Some(pending) if pending.expires_at_ms > now => pending.user_id,
                _ => {
                    challenges.remove(&challenge_hash);
                    return Err(AppError::unauthorized(
                        "that sign in has expired; enter your password again",
                    ));
                }
            }
        };

        if !self.check_second_factor(user_id, code, now)? {
            self.throttle.record_failure(client, now);
            let exhausted = self.count_code_attempt(&challenge_hash);
            tracing::info!(%client, user_id, "wrong sign in code");
            return Err(AppError::unauthorized(if exhausted {
                "too many wrong codes; enter your password again"
            } else {
                "that code is not right"
            }));
        }

        if let Ok(mut challenges) = self.challenges.lock() {
            challenges.remove(&challenge_hash);
        }
        self.throttle.record_success(client);

        let user = self
            .repository
            .find_user(user_id)?
            .ok_or_else(|| AppError::unauthorized("enter your password again"))?;
        let token = self.start_session(user.id)?;
        tracing::info!(email = %user.email, "logged in with a second factor");
        Ok(SignedIn { user, token })
    }

    /// Forget a pending sign in, when somebody goes back from the code step.
    pub fn abandon_challenge(&self, challenge: &str) {
        if let Ok(mut challenges) = self.challenges.lock() {
            challenges.remove(&hash_token(challenge));
        }
    }

    /// Whether the account has two factor sign in, and how many recovery codes are still unused.
    pub fn two_factor_status(&self, user_id: i64) -> AppResult<(bool, i64)> {
        let enabled = self.repository.find_totp(user_id)?.secret.is_some();
        let left = if enabled {
            self.repository.recovery_codes_left(user_id)?
        } else {
            0
        };
        Ok((enabled, left))
    }

    /// Start setting up an authenticator app: make a secret, keep it pending, and describe it for the QR
    /// code. Nothing changes for signing in until [`Self::enable_two_factor`] sees a code from it.
    pub fn begin_two_factor_setup(&self, user: &User) -> AppResult<TwoFactorSetup> {
        if self.repository.find_totp(user.id)?.secret.is_some() {
            return Err(AppError::conflict(
                "two factor sign in is already on; turn it off first to move it to a new app",
            ));
        }

        let secret = totp::generate_secret();
        self.repository.set_pending_totp(user.id, &secret)?;

        let otpauth_uri = totp::otpauth_uri(ISSUER, &user.email, &secret);
        let qr_svg = QrCode::new(otpauth_uri.as_bytes())
            .map_err(|error| AppError::internal(format!("could not draw the QR code: {error}")))?
            .render::<svg::Color>()
            .min_dimensions(200, 200)
            .quiet_zone(true)
            .build();

        Ok(TwoFactorSetup {
            secret_key: totp::grouped(&totp::base32_encode(&secret)),
            otpauth_uri,
            qr_svg,
        })
    }

    /// Confirm the app was set up by checking one code from it, then switch two factor sign in on and
    /// return the recovery codes. They are shown once; only their hashes are kept.
    pub fn enable_two_factor(&self, user_id: i64, code: &str) -> AppResult<Vec<String>> {
        let record = self.repository.find_totp(user_id)?;
        if record.secret.is_some() {
            return Err(AppError::conflict("two factor sign in is already on"));
        }
        let Some(pending) = record.pending_secret else {
            return Err(AppError::conflict(
                "start the setup again; there is no QR code waiting to be confirmed",
            ));
        };

        let Some(step) = totp::verify(&pending, code, now_ms(), None) else {
            return Err(AppError::bad_request(
                "that code is not right; check the app shows On Air Record and try the current code",
            ));
        };

        let codes = generate_recovery_codes();
        let hashes: Vec<Vec<u8>> = codes.iter().map(|code| hash_recovery_code(code)).collect();
        self.repository
            .enable_totp(user_id, &pending, step, &hashes)?;
        tracing::info!(user_id, "two factor sign in switched on");
        Ok(codes)
    }

    /// Switch your own two factor sign in off. Needs the password, so a browser left signed in cannot be
    /// used to remove the second factor.
    pub fn disable_two_factor(&self, user_id: i64, password: &str) -> AppResult<()> {
        self.confirm_password(user_id, password)?;
        self.repository.disable_totp(user_id)?;
        tracing::info!(user_id, "two factor sign in switched off");
        Ok(())
    }

    /// Replace your recovery codes, for when they are used up or may have been seen.
    pub fn regenerate_recovery_codes(
        &self,
        user_id: i64,
        password: &str,
    ) -> AppResult<Vec<String>> {
        self.confirm_password(user_id, password)?;
        if self.repository.find_totp(user_id)?.secret.is_none() {
            return Err(AppError::conflict(
                "two factor sign in is off, so there are no recovery codes",
            ));
        }

        let codes = generate_recovery_codes();
        let hashes: Vec<Vec<u8>> = codes.iter().map(|code| hash_recovery_code(code)).collect();
        self.repository.replace_recovery_codes(user_id, &hashes)?;
        Ok(codes)
    }

    /// An admin removing somebody else's second factor, because they lost their phone and their codes.
    pub fn reset_two_factor(&self, user_id: i64) -> AppResult<()> {
        if self.repository.find_user(user_id)?.is_none() {
            return Err(AppError::not_found(format!("no account with id {user_id}")));
        }
        self.repository.disable_totp(user_id)?;
        tracing::info!(user_id, "two factor sign in removed by an admin");
        Ok(())
    }

    /// Remove an account's second factor from the host. For the recovery command.
    pub fn reset_two_factor_by_email(&self, email: &str) -> AppResult<()> {
        let credentials = self
            .repository
            .find_credentials(email)?
            .ok_or_else(|| AppError::not_found(format!("no account uses {}", email.trim())))?;
        if !credentials.user.two_factor {
            return Err(AppError::conflict(format!(
                "{} does not have two factor sign in",
                credentials.user.email
            )));
        }
        self.repository.disable_totp(credentials.user.id)
    }

    fn confirm_password(&self, user_id: i64, password: &str) -> AppResult<()> {
        let credentials = self
            .repository
            .find_credentials_by_id(user_id)?
            .ok_or_else(|| AppError::unauthorized("sign in again"))?;
        if !verify_password(&credentials.password_hash, password) {
            return Err(AppError::bad_request("the password is not right"));
        }
        Ok(())
    }

    /// Check a code from the app, or failing that a recovery code. A six digit entry is only ever an app
    /// code; anything else is only ever a recovery code, so one cannot be mistaken for the other.
    fn check_second_factor(&self, user_id: i64, code: &str, now: i64) -> AppResult<bool> {
        let compact: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();

        if compact.len() == 6 && compact.chars().all(|c| c.is_ascii_digit()) {
            let record = self.repository.find_totp(user_id)?;
            let Some(secret) = record.secret else {
                return Ok(false);
            };
            return match totp::verify(&secret, &compact, now, record.last_step) {
                Some(step) => {
                    self.repository.record_totp_step(user_id, step)?;
                    Ok(true)
                }
                None => Ok(false),
            };
        }

        let used = self
            .repository
            .use_recovery_code(user_id, &hash_recovery_code(code))?;
        if used {
            tracing::info!(user_id, "signed in with a recovery code");
        }
        Ok(used)
    }

    fn start_challenge(&self, user_id: i64, now: i64) -> String {
        let token = generate_token();
        if let Ok(mut challenges) = self.challenges.lock() {
            challenges.retain(|_, pending| pending.expires_at_ms > now);
            challenges.insert(
                hash_token(&token),
                Challenge {
                    user_id,
                    expires_at_ms: now + CHALLENGE_TTL_MS,
                    attempts: 0,
                },
            );
        }
        token
    }

    /// Count a wrong code against a challenge. True when that used up its attempts and it was dropped.
    fn count_code_attempt(&self, challenge_hash: &[u8]) -> bool {
        let Ok(mut challenges) = self.challenges.lock() else {
            return true;
        };
        let exhausted = match challenges.get_mut(challenge_hash) {
            Some(pending) => {
                pending.attempts += 1;
                pending.attempts >= MAX_CODE_ATTEMPTS
            }
            None => true,
        };
        if exhausted {
            challenges.remove(challenge_hash);
        }
        exhausted
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

/// Recovery codes, as `xxxxx-xxxxx` from an alphabet with no look alike characters. Ten characters from
/// 31 is about 49 bits each, far past guessing within the login throttle, so a plain SHA-256 of each is a
/// safe way to store them.
fn generate_recovery_codes() -> Vec<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    (0..RECOVERY_CODE_COUNT)
        .map(|_| {
            let mut bytes = [0u8; 10];
            OsRng.fill_bytes(&mut bytes);
            let chars: String = bytes
                .iter()
                .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
                .collect();
            format!("{}-{}", &chars[..5], &chars[5..])
        })
        .collect()
}

/// Hash a recovery code as typed: case, spaces and the dash do not matter, so a code read off paper is
/// accepted however it was copied.
fn hash_recovery_code(typed: &str) -> Vec<u8> {
    let normalised: String = typed
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    Sha256::digest(normalised.as_bytes()).to_vec()
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
            .expect("login")
            .signed_in()
            .expect("no second factor on this account");
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
            .expect("second login")
            .signed_in()
            .expect("no second factor on this account");

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

    /// An account with an authenticator app set up, as a phone would have it: the service, the signed
    /// in session, the secret the phone holds, and the recovery codes shown at setup.
    struct TwoFactorAccount {
        service: AuthService,
        user: User,
        secret: Vec<u8>,
        recovery_codes: Vec<String>,
    }

    fn with_two_factor() -> TwoFactorAccount {
        let service = service();
        let signed_in = service
            .set_up("owner@example.com", "a long password")
            .expect("setup");
        let setup = service
            .begin_two_factor_setup(&signed_in.user)
            .expect("begin");
        let secret = totp::base32_decode(&setup.secret_key);
        let code = format!("{:06}", totp::code_at(&secret, totp::step_at(now_ms())));
        let recovery_codes = service
            .enable_two_factor(signed_in.user.id, &code)
            .expect("enable");
        TwoFactorAccount {
            service,
            user: signed_in.user,
            secret,
            recovery_codes,
        }
    }

    /// The code a phone shows one step from now: inside the accepted window, and newer than the code
    /// that confirmed setup, so it is not refused as a replay.
    fn next_code(secret: &[u8]) -> String {
        format!("{:06}", totp::code_at(secret, totp::step_at(now_ms()) + 1))
    }

    fn challenge_for(account: &TwoFactorAccount) -> String {
        match account
            .service
            .log_in(CLIENT, "owner@example.com", "a long password")
            .expect("password")
        {
            LoginOutcome::SecondFactorRequired { challenge } => challenge,
            LoginOutcome::SignedIn(_) => panic!("the password alone should not be enough"),
        }
    }

    #[test]
    fn setup_shows_a_scannable_secret_and_changes_nothing_until_confirmed() {
        let service = service();
        let signed_in = service
            .set_up("owner@example.com", "a long password")
            .expect("setup");
        let setup = service
            .begin_two_factor_setup(&signed_in.user)
            .expect("begin");

        assert!(setup
            .otpauth_uri
            .starts_with("otpauth://totp/On%20Air%20Record:owner@example.com?secret="));
        assert!(setup.qr_svg.contains("<svg"));
        assert_eq!(
            service
                .two_factor_status(signed_in.user.id)
                .expect("status"),
            (false, 0)
        );

        // Not confirmed yet, so the password alone still signs in.
        let outcome = service
            .log_in(CLIENT, "owner@example.com", "a long password")
            .expect("login");
        assert!(outcome.signed_in().is_some());

        assert!(service
            .enable_two_factor(signed_in.user.id, "000000")
            .is_err());
    }

    #[test]
    fn once_on_the_password_alone_is_not_enough() {
        let account = with_two_factor();
        assert_eq!(account.recovery_codes.len(), RECOVERY_CODE_COUNT);
        assert_eq!(
            account
                .service
                .two_factor_status(account.user.id)
                .expect("status"),
            (true, RECOVERY_CODE_COUNT as i64)
        );

        let challenge = challenge_for(&account);
        assert!(account.service.has_pending_challenge(Some(&challenge)));

        let signed_in = account
            .service
            .verify_second_factor(CLIENT, &challenge, &next_code(&account.secret))
            .expect("code");
        assert_eq!(signed_in.user.id, account.user.id);
        assert!(signed_in.user.two_factor);
        assert!(!account.service.has_pending_challenge(Some(&challenge)));
    }

    #[test]
    fn a_code_that_signed_somebody_in_cannot_sign_in_again() {
        let account = with_two_factor();
        let code = next_code(&account.secret);

        let first = challenge_for(&account);
        account
            .service
            .verify_second_factor(CLIENT, &first, &code)
            .expect("first use");

        let second = challenge_for(&account);
        assert!(account
            .service
            .verify_second_factor(CLIENT, &second, &code)
            .is_err());
    }

    #[test]
    fn a_recovery_code_works_once_however_it_is_typed() {
        let account = with_two_factor();
        let code = account.recovery_codes[0].to_uppercase().replace('-', " ");

        let challenge = challenge_for(&account);
        account
            .service
            .verify_second_factor(CLIENT, &challenge, &code)
            .expect("recovery code");
        assert_eq!(
            account
                .service
                .two_factor_status(account.user.id)
                .expect("status")
                .1,
            RECOVERY_CODE_COUNT as i64 - 1
        );

        let again = challenge_for(&account);
        assert!(account
            .service
            .verify_second_factor(CLIENT, &again, &account.recovery_codes[0])
            .is_err());
    }

    #[test]
    fn five_wrong_codes_end_the_pending_sign_in() {
        let account = with_two_factor();
        let challenge = challenge_for(&account);

        for _ in 0..MAX_CODE_ATTEMPTS {
            assert!(account
                .service
                .verify_second_factor(CLIENT, &challenge, "000000")
                .is_err());
        }
        // Even the right code is too late now; the password has to be typed again.
        let late =
            account
                .service
                .verify_second_factor(CLIENT, &challenge, &next_code(&account.secret));
        assert!(late.is_err());
        assert!(!account.service.has_pending_challenge(Some(&challenge)));
    }

    #[test]
    fn wrong_codes_count_towards_the_address_lockout() {
        let account = with_two_factor();
        // The failed attempts are spread over fresh challenges, so it is the address, not the challenge,
        // that runs out.
        for _ in 0..MAX_FAILURES {
            let challenge = challenge_for(&account);
            let _ = account
                .service
                .verify_second_factor(CLIENT, &challenge, "000000");
        }
        assert!(matches!(
            account
                .service
                .log_in(CLIENT, "owner@example.com", "a long password"),
            Err(AppError::TooManyRequests(_))
        ));
    }

    #[test]
    fn an_unknown_or_abandoned_challenge_is_refused() {
        let account = with_two_factor();
        assert!(account
            .service
            .verify_second_factor(CLIENT, "made-up", &next_code(&account.secret))
            .is_err());

        let challenge = challenge_for(&account);
        account.service.abandon_challenge(&challenge);
        assert!(account
            .service
            .verify_second_factor(CLIENT, &challenge, &next_code(&account.secret))
            .is_err());
    }

    #[test]
    fn turning_it_off_needs_the_password() {
        let account = with_two_factor();
        assert!(account
            .service
            .disable_two_factor(account.user.id, "wrong password")
            .is_err());

        account
            .service
            .disable_two_factor(account.user.id, "a long password")
            .expect("disable");
        assert_eq!(
            account
                .service
                .two_factor_status(account.user.id)
                .expect("status"),
            (false, 0)
        );
        assert!(account
            .service
            .log_in(CLIENT, "owner@example.com", "a long password")
            .expect("login")
            .signed_in()
            .is_some());
    }

    #[test]
    fn new_recovery_codes_replace_the_old_ones() {
        let account = with_two_factor();
        assert!(account
            .service
            .regenerate_recovery_codes(account.user.id, "wrong password")
            .is_err());

        let fresh = account
            .service
            .regenerate_recovery_codes(account.user.id, "a long password")
            .expect("regenerate");
        assert_eq!(fresh.len(), RECOVERY_CODE_COUNT);

        let challenge = challenge_for(&account);
        assert!(account
            .service
            .verify_second_factor(CLIENT, &challenge, &account.recovery_codes[1])
            .is_err());
        let challenge = challenge_for(&account);
        assert!(account
            .service
            .verify_second_factor(CLIENT, &challenge, &fresh[0])
            .is_ok());
    }

    #[test]
    fn a_second_setup_is_refused_while_it_is_on() {
        let account = with_two_factor();
        assert!(matches!(
            account.service.begin_two_factor_setup(&account.user),
            Err(AppError::Conflict(_))
        ));
    }

    #[test]
    fn an_admin_or_the_host_can_remove_a_lost_second_factor() {
        let account = with_two_factor();
        account
            .service
            .reset_two_factor(account.user.id)
            .expect("admin reset");
        assert!(
            !account
                .service
                .two_factor_status(account.user.id)
                .expect("status")
                .0
        );

        assert!(matches!(
            account
                .service
                .reset_two_factor_by_email("owner@example.com"),
            Err(AppError::Conflict(_))
        ));
        assert!(account.service.reset_two_factor(9_999).is_err());
    }
}
