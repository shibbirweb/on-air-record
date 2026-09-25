//! Persistence for the login mode, accounts, and login sessions.
//!
//! The two rules that must never be broken by a race, "setup happens once" and "there is always an admin",
//! are checked inside the same transaction as the write they guard. The connection is a single mutex, so
//! two admins clicking at the same moment are serialised rather than both passing a check made earlier.

use std::sync::Arc;

use rusqlite::{Connection, ErrorCode, OptionalExtension, Row};

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::models::{AuthMode, Role, User};
use crate::util::time::now_ms;

const USER_COLUMNS: &str = "id, email, role, created_at_ms, totp_secret IS NOT NULL";

/// An account together with what is needed to check its password. Only the auth service sees this.
pub struct Credentials {
    pub user: User,
    pub password_hash: String,
}

/// A live login session, resolved to its account.
pub struct SessionRecord {
    pub user: User,
    pub last_seen_at_ms: i64,
}

/// An account's authenticator state. Only the auth service sees this.
#[derive(Debug, Default)]
pub struct TotpRecord {
    /// The confirmed secret, present once two factor sign in is on.
    pub secret: Option<Vec<u8>>,
    /// A secret shown as a QR code but not yet confirmed with a code.
    pub pending_secret: Option<Vec<u8>>,
    /// The newest time step a code was accepted for, so it cannot be used again.
    pub last_step: Option<i64>,
}

pub struct AuthRepository {
    database: Arc<Database>,
}

impl AuthRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn mode(&self) -> AppResult<AuthMode> {
        self.database.with_connection(read_mode)
    }

    /// Record that the install stays open. Only allowed while the question is still unanswered, so a
    /// stale popup in a second browser cannot switch accounts off again.
    pub fn choose_open(&self) -> AppResult<()> {
        self.database.with_connection(|conn| {
            let changed = conn.execute(
                "UPDATE auth_state SET mode = 'open', decided_at_ms = ?1
                 WHERE id = 1 AND mode = 'undecided'",
                rusqlite::params![now_ms()],
            )?;
            if changed == 0 {
                return Err(AppError::conflict("the login choice has already been made"));
            }
            Ok(())
        })
    }

    /// Create the first admin and switch accounts on, in one transaction.
    ///
    /// Refused once accounts are on, which is what stops a second visitor creating another admin after
    /// the first one has finished. Accounts left over from an earlier period with accounts on are removed,
    /// because whoever is setting up now is starting afresh.
    pub fn set_up_first_admin(&self, email: &str, password_hash: &str) -> AppResult<User> {
        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;

            if read_mode(&transaction)? == AuthMode::Accounts {
                return Err(AppError::conflict("accounts are already set up"));
            }

            transaction.execute("DELETE FROM users", [])?;
            let user = insert_user(&transaction, email, password_hash, Role::Admin)?;
            transaction.execute(
                "UPDATE auth_state SET mode = 'accounts', decided_at_ms = ?1 WHERE id = 1",
                rusqlite::params![now_ms()],
            )?;

            transaction.commit()?;
            Ok(user)
        })
    }

    /// Switch accounts off and forget every account and session. Used by the recovery command.
    pub fn disable_accounts(&self) -> AppResult<()> {
        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute("DELETE FROM users", [])?;
            transaction.execute(
                "UPDATE auth_state SET mode = 'open', decided_at_ms = ?1 WHERE id = 1",
                rusqlite::params![now_ms()],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn create_user(&self, email: &str, password_hash: &str, role: Role) -> AppResult<User> {
        self.database
            .with_connection(|conn| insert_user(conn, email, password_hash, role))
    }

    pub fn find_credentials(&self, email: &str) -> AppResult<Option<Credentials>> {
        self.database.with_connection(|conn| {
            conn.query_row(
                &format!("SELECT {USER_COLUMNS}, password_hash FROM users WHERE email = ?1"),
                rusqlite::params![email.trim()],
                |row| {
                    Ok(Credentials {
                        user: map_user(row)?,
                        password_hash: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn find_credentials_by_id(&self, user_id: i64) -> AppResult<Option<Credentials>> {
        self.database.with_connection(|conn| {
            conn.query_row(
                &format!("SELECT {USER_COLUMNS}, password_hash FROM users WHERE id = ?1"),
                rusqlite::params![user_id],
                |row| {
                    Ok(Credentials {
                        user: map_user(row)?,
                        password_hash: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn find_user(&self, user_id: i64) -> AppResult<Option<User>> {
        self.database.with_connection(|conn| {
            conn.query_row(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"),
                rusqlite::params![user_id],
                map_user,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    /// Every account, admins first, then alphabetically, which is the order the accounts page lists them.
    pub fn list_users(&self) -> AppResult<Vec<User>> {
        self.database.with_connection(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {USER_COLUMNS} FROM users
                 ORDER BY CASE role WHEN 'admin' THEN 0 ELSE 1 END, email COLLATE NOCASE"
            ))?;
            let rows = statement.query_map([], map_user)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
        })
    }

    /// Change an account's role, refusing to demote the last admin.
    pub fn update_role(&self, user_id: i64, role: Role) -> AppResult<User> {
        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;
            let current = find_role(&transaction, user_id)?;

            if current == Role::Admin && role != Role::Admin && count_admins(&transaction)? <= 1 {
                return Err(AppError::conflict(
                    "this is the only admin, so it cannot become a listener; make another admin first",
                ));
            }

            transaction.execute(
                "UPDATE users SET role = ?1 WHERE id = ?2",
                rusqlite::params![role.as_str(), user_id],
            )?;
            let user = transaction.query_row(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"),
                rusqlite::params![user_id],
                map_user,
            )?;
            transaction.commit()?;
            Ok(user)
        })
    }

    pub fn update_password(&self, user_id: i64, password_hash: &str) -> AppResult<()> {
        self.database.with_connection(|conn| {
            let changed = conn.execute(
                "UPDATE users SET password_hash = ?1, password_changed_at_ms = ?2 WHERE id = ?3",
                rusqlite::params![password_hash, now_ms(), user_id],
            )?;
            if changed == 0 {
                return Err(AppError::not_found(format!("no account with id {user_id}")));
            }
            Ok(())
        })
    }

    /// Remove an account and, through the foreign key, every session it had. Refuses the last admin.
    pub fn delete_user(&self, user_id: i64) -> AppResult<()> {
        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;

            if find_role(&transaction, user_id)? == Role::Admin && count_admins(&transaction)? <= 1
            {
                return Err(AppError::conflict(
                    "this is the only admin, so it cannot be removed; make another admin first",
                ));
            }

            transaction.execute(
                "DELETE FROM users WHERE id = ?1",
                rusqlite::params![user_id],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn create_session(
        &self,
        token_hash: &[u8],
        user_id: i64,
        expires_at_ms: i64,
    ) -> AppResult<()> {
        let now = now_ms();
        self.database.with_connection(|conn| {
            conn.execute(
                "INSERT INTO auth_sessions (token_hash, user_id, created_at_ms, expires_at_ms, last_seen_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?3)",
                rusqlite::params![token_hash, user_id, now, expires_at_ms],
            )?;
            Ok(())
        })
    }

    /// The account behind an unexpired session, if there is one.
    pub fn find_session(&self, token_hash: &[u8], now: i64) -> AppResult<Option<SessionRecord>> {
        self.database.with_connection(|conn| {
            conn.query_row(
                "SELECT users.id, users.email, users.role, users.created_at_ms, users.totp_secret IS NOT NULL, auth_sessions.last_seen_at_ms
                 FROM auth_sessions JOIN users ON users.id = auth_sessions.user_id
                 WHERE auth_sessions.token_hash = ?1 AND auth_sessions.expires_at_ms > ?2",
                rusqlite::params![token_hash, now],
                |row| {
                    Ok(SessionRecord {
                        user: map_user(row)?,
                        last_seen_at_ms: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn touch_session(&self, token_hash: &[u8], now: i64) -> AppResult<()> {
        self.database.with_connection(|conn| {
            conn.execute(
                "UPDATE auth_sessions SET last_seen_at_ms = ?1 WHERE token_hash = ?2",
                rusqlite::params![now, token_hash],
            )?;
            Ok(())
        })
    }

    pub fn delete_session(&self, token_hash: &[u8]) -> AppResult<()> {
        self.database.with_connection(|conn| {
            conn.execute(
                "DELETE FROM auth_sessions WHERE token_hash = ?1",
                rusqlite::params![token_hash],
            )?;
            Ok(())
        })
    }

    /// Sign an account out everywhere, optionally keeping the session making the request.
    pub fn delete_sessions_for_user(&self, user_id: i64, keep: Option<&[u8]>) -> AppResult<()> {
        self.database.with_connection(|conn| {
            match keep {
                Some(token_hash) => conn.execute(
                    "DELETE FROM auth_sessions WHERE user_id = ?1 AND token_hash != ?2",
                    rusqlite::params![user_id, token_hash],
                )?,
                None => conn.execute(
                    "DELETE FROM auth_sessions WHERE user_id = ?1",
                    rusqlite::params![user_id],
                )?,
            };
            Ok(())
        })
    }

    pub fn delete_expired_sessions(&self, now: i64) -> AppResult<usize> {
        self.database.with_connection(|conn| {
            Ok(conn.execute(
                "DELETE FROM auth_sessions WHERE expires_at_ms <= ?1",
                rusqlite::params![now],
            )?)
        })
    }

    pub fn find_totp(&self, user_id: i64) -> AppResult<TotpRecord> {
        self.database.with_connection(|conn| {
            conn.query_row(
                "SELECT totp_secret, totp_pending_secret, totp_last_step FROM users WHERE id = ?1",
                rusqlite::params![user_id],
                |row| {
                    Ok(TotpRecord {
                        secret: row.get(0)?,
                        pending_secret: row.get(1)?,
                        last_step: row.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::not_found(format!("no account with id {user_id}")))
        })
    }

    /// Remember a secret that has been shown but not yet confirmed. Replaces any earlier one, so starting
    /// setup again simply shows a new QR code.
    pub fn set_pending_totp(&self, user_id: i64, secret: &[u8]) -> AppResult<()> {
        self.database.with_connection(|conn| {
            conn.execute(
                "UPDATE users SET totp_pending_secret = ?1 WHERE id = ?2",
                rusqlite::params![secret, user_id],
            )?;
            Ok(())
        })
    }

    /// Switch two factor sign in on with a confirmed secret and a fresh set of recovery codes, in one
    /// transaction, so there is never a moment with the secret but without the codes to get back in.
    pub fn enable_totp(
        &self,
        user_id: i64,
        secret: &[u8],
        confirmed_step: i64,
        recovery_code_hashes: &[Vec<u8>],
    ) -> AppResult<()> {
        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute(
                "UPDATE users
                 SET totp_secret = ?1, totp_pending_secret = NULL, totp_last_step = ?2
                 WHERE id = ?3",
                rusqlite::params![secret, confirmed_step, user_id],
            )?;
            write_recovery_codes(&transaction, user_id, recovery_code_hashes)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn record_totp_step(&self, user_id: i64, step: i64) -> AppResult<()> {
        self.database.with_connection(|conn| {
            conn.execute(
                "UPDATE users SET totp_last_step = ?1 WHERE id = ?2",
                rusqlite::params![step, user_id],
            )?;
            Ok(())
        })
    }

    /// Switch two factor sign in off and forget the secret and every recovery code.
    pub fn disable_totp(&self, user_id: i64) -> AppResult<()> {
        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute(
                "UPDATE users
                 SET totp_secret = NULL, totp_pending_secret = NULL, totp_last_step = NULL
                 WHERE id = ?1",
                rusqlite::params![user_id],
            )?;
            transaction.execute(
                "DELETE FROM recovery_codes WHERE user_id = ?1",
                rusqlite::params![user_id],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn replace_recovery_codes(&self, user_id: i64, code_hashes: &[Vec<u8>]) -> AppResult<()> {
        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;
            write_recovery_codes(&transaction, user_id, code_hashes)?;
            transaction.commit()?;
            Ok(())
        })
    }

    /// Spend a recovery code. True only the first time a given code is used.
    pub fn use_recovery_code(&self, user_id: i64, code_hash: &[u8]) -> AppResult<bool> {
        self.database.with_connection(|conn| {
            let changed = conn.execute(
                "UPDATE recovery_codes SET used_at_ms = ?1
                 WHERE user_id = ?2 AND code_hash = ?3 AND used_at_ms IS NULL",
                rusqlite::params![now_ms(), user_id, code_hash],
            )?;
            Ok(changed == 1)
        })
    }

    pub fn recovery_codes_left(&self, user_id: i64) -> AppResult<i64> {
        self.database.with_connection(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM recovery_codes WHERE user_id = ?1 AND used_at_ms IS NULL",
                rusqlite::params![user_id],
                |row| row.get(0),
            )?)
        })
    }
}

/// Replace an account's recovery codes with new ones. Old codes stop working, used or not.
fn write_recovery_codes(conn: &Connection, user_id: i64, code_hashes: &[Vec<u8>]) -> AppResult<()> {
    conn.execute(
        "DELETE FROM recovery_codes WHERE user_id = ?1",
        rusqlite::params![user_id],
    )?;
    for code_hash in code_hashes {
        conn.execute(
            "INSERT INTO recovery_codes (user_id, code_hash) VALUES (?1, ?2)",
            rusqlite::params![user_id, code_hash],
        )?;
    }
    Ok(())
}

fn read_mode(conn: &Connection) -> AppResult<AuthMode> {
    let raw: String = conn.query_row("SELECT mode FROM auth_state WHERE id = 1", [], |row| {
        row.get(0)
    })?;
    // The column has a CHECK constraint, so anything else means the file was edited by hand. Asking the
    // question again is the safe reading: it can never leave an install open that meant to be closed
    // without the owner seeing the prompt.
    Ok(AuthMode::parse(&raw).unwrap_or(AuthMode::Undecided))
}

fn insert_user(conn: &Connection, email: &str, password_hash: &str, role: Role) -> AppResult<User> {
    let now = now_ms();
    let email = email.trim();

    let inserted = conn.execute(
        "INSERT INTO users (email, password_hash, role, created_at_ms, password_changed_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        rusqlite::params![email, password_hash, role.as_str(), now],
    );

    match inserted {
        Ok(_) => Ok(User {
            id: conn.last_insert_rowid(),
            email: email.to_string(),
            role,
            created_at_ms: now,
            two_factor: false,
        }),
        Err(rusqlite::Error::SqliteFailure(failure, _))
            if failure.code == ErrorCode::ConstraintViolation =>
        {
            Err(AppError::conflict(
                "an account with that email already exists",
            ))
        }
        Err(error) => Err(error.into()),
    }
}

fn find_role(conn: &Connection, user_id: i64) -> AppResult<Role> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT role FROM users WHERE id = ?1",
            rusqlite::params![user_id],
            |row| row.get(0),
        )
        .optional()?;

    match raw {
        Some(raw) => Ok(Role::parse(&raw).unwrap_or(Role::Listener)),
        None => Err(AppError::not_found(format!("no account with id {user_id}"))),
    }
}

fn count_admins(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM users WHERE role = 'admin'",
        [],
        |row| row.get(0),
    )?)
}

fn map_user(row: &Row<'_>) -> rusqlite::Result<User> {
    let role: String = row.get(2)?;
    Ok(User {
        id: row.get(0)?,
        email: row.get(1)?,
        // A role the code does not know gets the least privilege rather than failing the whole query.
        role: Role::parse(&role).unwrap_or(Role::Listener),
        created_at_ms: row.get(3)?,
        two_factor: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> AuthRepository {
        AuthRepository::new(Arc::new(Database::open_in_memory().expect("database")))
    }

    #[test]
    fn a_fresh_install_is_undecided() {
        assert_eq!(repository().mode().expect("mode"), AuthMode::Undecided);
    }

    #[test]
    fn staying_open_can_only_be_chosen_once() {
        let repository = repository();
        repository.choose_open().expect("first choice");
        assert_eq!(repository.mode().expect("mode"), AuthMode::Open);
        assert!(repository.choose_open().is_err());
    }

    #[test]
    fn setup_creates_one_admin_and_then_refuses() {
        let repository = repository();
        let admin = repository
            .set_up_first_admin("owner@example.com", "hash")
            .expect("setup");
        assert_eq!(admin.role, Role::Admin);
        assert_eq!(repository.mode().expect("mode"), AuthMode::Accounts);

        assert!(repository
            .set_up_first_admin("intruder@example.com", "hash")
            .is_err());
        assert_eq!(repository.list_users().expect("users").len(), 1);
    }

    #[test]
    fn accounts_can_be_set_up_after_choosing_to_stay_open() {
        let repository = repository();
        repository.choose_open().expect("open");
        repository
            .set_up_first_admin("owner@example.com", "hash")
            .expect("setup from settings");
        assert_eq!(repository.mode().expect("mode"), AuthMode::Accounts);
    }

    #[test]
    fn emails_are_unique_regardless_of_case() {
        let repository = repository();
        repository
            .create_user("Someone@Example.com", "hash", Role::Listener)
            .expect("first");
        let duplicate = repository.create_user("someone@example.com", "hash", Role::Listener);
        assert!(matches!(duplicate, Err(AppError::Conflict(_))));

        let found = repository
            .find_credentials("SOMEONE@example.COM")
            .expect("lookup");
        assert!(found.is_some());
    }

    #[test]
    fn the_last_admin_can_be_neither_demoted_nor_removed() {
        let repository = repository();
        let admin = repository
            .set_up_first_admin("owner@example.com", "hash")
            .expect("setup");

        assert!(repository.update_role(admin.id, Role::Listener).is_err());
        assert!(repository.delete_user(admin.id).is_err());

        let second = repository
            .create_user("second@example.com", "hash", Role::Admin)
            .expect("second admin");
        repository
            .update_role(admin.id, Role::Listener)
            .expect("now allowed");
        assert!(repository.delete_user(second.id).is_err());
    }

    #[test]
    fn sessions_resolve_expire_and_disappear_with_their_account() {
        let repository = repository();
        let admin = repository
            .set_up_first_admin("owner@example.com", "hash")
            .expect("setup");
        let listener = repository
            .create_user("listener@example.com", "hash", Role::Listener)
            .expect("listener");

        repository
            .create_session(b"token-a", listener.id, 1_000)
            .expect("session");
        let found = repository.find_session(b"token-a", 500).expect("lookup");
        assert_eq!(found.map(|record| record.user.id), Some(listener.id));
        assert!(repository
            .find_session(b"token-a", 1_000)
            .expect("expired")
            .is_none());

        repository
            .create_session(b"token-b", listener.id, 10_000)
            .expect("session");
        repository.delete_user(listener.id).expect("delete");
        assert!(repository
            .find_session(b"token-b", 500)
            .expect("gone")
            .is_none());

        repository
            .create_session(b"keep", admin.id, 10_000)
            .expect("session");
        repository
            .create_session(b"drop", admin.id, 10_000)
            .expect("session");
        repository
            .delete_sessions_for_user(admin.id, Some(b"keep"))
            .expect("sign out elsewhere");
        assert!(repository.find_session(b"keep", 0).expect("kept").is_some());
        assert!(repository
            .find_session(b"drop", 0)
            .expect("dropped")
            .is_none());
    }

    #[test]
    fn disabling_accounts_forgets_everyone() {
        let repository = repository();
        let admin = repository
            .set_up_first_admin("owner@example.com", "hash")
            .expect("setup");
        repository
            .create_session(b"token", admin.id, 10_000)
            .expect("session");

        repository.disable_accounts().expect("disable");
        assert_eq!(repository.mode().expect("mode"), AuthMode::Open);
        assert!(repository.list_users().expect("users").is_empty());
        assert!(repository
            .find_session(b"token", 0)
            .expect("gone")
            .is_none());
    }
}
