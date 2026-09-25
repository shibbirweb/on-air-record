//! Accounts, roles, and whether the install requires a login at all.

use serde::Serialize;

/// What an account is allowed to do.
///
/// Two roles rather than a permission matrix, because there are exactly two kinds of people using a
/// recorder: whoever runs it, and whoever listens to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Controls capture, settings, bookmarks and accounts.
    Admin,
    /// Listens live, scrubs history and exports. Changes nothing.
    Listener,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Listener => "listener",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "admin" => Some(Self::Admin),
            "listener" => Some(Self::Listener),
            _ => None,
        }
    }
}

/// Whether the install asks for a login.
///
/// Chosen once, by whoever first opens the page. `Undecided` behaves exactly like `Open`, so an upgrade
/// never locks anybody out; it only means the question has not been answered yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    Undecided,
    Open,
    Accounts,
}

impl AuthMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Undecided => "undecided",
            Self::Open => "open",
            Self::Accounts => "accounts",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "undecided" => Some(Self::Undecided),
            "open" => Some(Self::Open),
            "accounts" => Some(Self::Accounts),
            _ => None,
        }
    }

    pub fn requires_login(self) -> bool {
        matches!(self, Self::Accounts)
    }
}

/// An account as the rest of the application sees it. The password hash never leaves the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub role: Role,
    pub created_at_ms: i64,
}

/// What a route needs from whoever is calling it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Listening, scrubbing, exporting and reading state.
    Listen,
    /// Anything that changes the recorder, its settings, its bookmarks or its accounts.
    Administer,
}

/// Why a request was turned away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// Nobody is signed in. The client should show the login page.
    SignInRequired,
    /// Somebody is signed in, but their role does not allow this.
    NotAllowed,
}

/// The single access rule, kept free of HTTP so it can be tested exhaustively.
///
/// Without accounts everybody may do everything, which is the documented meaning of choosing to keep the
/// install open. With accounts, a listener may listen and an admin may do anything.
pub fn authorize(mode: AuthMode, user: Option<&User>, needed: Access) -> Result<(), Denied> {
    if !mode.requires_login() {
        return Ok(());
    }

    let Some(user) = user else {
        return Err(Denied::SignInRequired);
    };

    match (needed, user.role) {
        (Access::Listen, _) => Ok(()),
        (Access::Administer, Role::Admin) => Ok(()),
        (Access::Administer, Role::Listener) => Err(Denied::NotAllowed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(role: Role) -> User {
        User {
            id: 1,
            email: "someone@example.com".to_string(),
            role,
            created_at_ms: 0,
        }
    }

    #[test]
    fn an_open_or_undecided_install_lets_anyone_do_anything() {
        for mode in [AuthMode::Undecided, AuthMode::Open] {
            assert_eq!(authorize(mode, None, Access::Administer), Ok(()));
            assert_eq!(authorize(mode, None, Access::Listen), Ok(()));
        }
    }

    #[test]
    fn with_accounts_nobody_signed_in_is_asked_to_sign_in() {
        assert_eq!(
            authorize(AuthMode::Accounts, None, Access::Listen),
            Err(Denied::SignInRequired)
        );
        assert_eq!(
            authorize(AuthMode::Accounts, None, Access::Administer),
            Err(Denied::SignInRequired)
        );
    }

    #[test]
    fn a_listener_may_listen_but_not_administer() {
        let listener = user(Role::Listener);
        assert_eq!(
            authorize(AuthMode::Accounts, Some(&listener), Access::Listen),
            Ok(())
        );
        assert_eq!(
            authorize(AuthMode::Accounts, Some(&listener), Access::Administer),
            Err(Denied::NotAllowed)
        );
    }

    #[test]
    fn an_admin_may_do_anything() {
        let admin = user(Role::Admin);
        assert_eq!(
            authorize(AuthMode::Accounts, Some(&admin), Access::Listen),
            Ok(())
        );
        assert_eq!(
            authorize(AuthMode::Accounts, Some(&admin), Access::Administer),
            Ok(())
        );
    }

    #[test]
    fn stored_names_round_trip() {
        for role in [Role::Admin, Role::Listener] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        for mode in [AuthMode::Undecided, AuthMode::Open, AuthMode::Accounts] {
            assert_eq!(AuthMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(Role::parse("root"), None);
    }
}
