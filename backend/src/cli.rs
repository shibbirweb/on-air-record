//! One off maintenance commands, run on the host instead of starting the service.
//!
//! These are the way back in for somebody locked out of the web interface: there is no mail server to
//! send a reset link, so having a shell on the machine is what proves you own it. They open the same
//! database as the service and can run while it is up, because every session is checked against the
//! database on each request, so a reset or a disable takes effect immediately.

use std::sync::Arc;

use crate::config::{AppConfig, AuthCommand, Command};
use crate::db::Database;
use crate::error::AppResult;
use crate::health_probe;
use crate::models::activity::{ActivityEvent, Actor, Origin};
use crate::repositories::{ActivityRepository, AuthRepository};
use crate::services::{ActivityService, AuthService};

pub fn run(config: &AppConfig, command: Command) -> AppResult<()> {
    match command {
        // Before the database is opened: a probe that runs every thirty seconds must not touch SQLite,
        // and must work with a data directory it cannot write to.
        Command::Health => health_probe::run(config),
        Command::Auth(command) => run_auth(config, command),
    }
}

fn run_auth(config: &AppConfig, command: AuthCommand) -> AppResult<()> {
    let database = Arc::new(Database::open(&config.database_path())?);
    let activity = ActivityService::new(Arc::new(ActivityRepository::new(database.clone())));
    let auth = AuthService::new(Arc::new(AuthRepository::new(database)));
    // Logged as done on the host, with no address: shell access is what proved who it was.
    let log = |event| activity.record(Actor::Host, &Origin::default(), event);

    match command {
        AuthCommand::ResetPassword { email } => {
            let account = account_email(&auth, &email);
            let password = auth.reset_password_by_email(&email)?;
            if let Some(email) = account {
                log(ActivityEvent::PasswordSet { email });
            }
            println!("New password for {}: {password}", email.trim());
            println!("Every session of that account has been signed out.");
            println!("Sign in with it, then change it under Account settings.");
        }
        AuthCommand::ResetTwoFactor { email } => {
            let account = account_email(&auth, &email);
            auth.reset_two_factor_by_email(&email)?;
            if let Some(email) = account {
                log(ActivityEvent::TwoFactorRemoved { email });
            }
            println!("Two factor sign in is off for {}.", email.trim());
            println!("They sign in with their password alone, and can set up a new app under Account settings.");
        }
        AuthCommand::Disable => {
            auth.disable_accounts()?;
            log(ActivityEvent::AccountsDisabled);
            println!("Accounts are switched off and every account has been deleted.");
            println!("Anyone who can reach the page can now use it without signing in.");
            println!("Switch accounts back on from the settings page.");
        }
    }

    Ok(())
}

/// The account's email as stored, for the log, however the command's argument was typed.
fn account_email(auth: &AuthService, typed: &str) -> Option<String> {
    let typed = typed.trim();
    auth.list_users()
        .ok()?
        .into_iter()
        .find(|user| user.email.eq_ignore_ascii_case(typed))
        .map(|user| user.email)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AuthMode;

    /// A data directory on disk, because the commands are meant to reach the service's database through
    /// the file, alongside whatever connection the running service holds.
    struct TempData {
        config: AppConfig,
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.config.data_dir);
        }
    }

    fn temp_data(name: &str) -> TempData {
        let data_dir = std::env::temp_dir().join(format!("oar-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data_dir);
        TempData {
            config: AppConfig {
                data_dir,
                ..AppConfig::default()
            },
        }
    }

    /// Stands in for the running service: its own connection to the same file.
    fn service(config: &AppConfig) -> AuthService {
        let database = Arc::new(Database::open(&config.database_path()).expect("database"));
        AuthService::new(Arc::new(AuthRepository::new(database)))
    }

    fn reset(email: &str) -> Command {
        Command::Auth(AuthCommand::ResetPassword {
            email: email.to_string(),
        })
    }

    /// The activity log as the running service would read it.
    fn activity_log(config: &AppConfig) -> Vec<crate::models::activity::ActivityEntry> {
        let database = Arc::new(Database::open(&config.database_path()).expect("database"));
        crate::repositories::ActivityRepository::new(database)
            .list(&crate::models::activity::ActivityQuery {
                limit: 50,
                ..Default::default()
            })
            .expect("list")
    }

    #[test]
    fn each_recovery_command_is_logged_as_done_on_the_host() {
        use crate::models::activity::{ActivityEvent, Actor};
        use crate::services::totp;
        use crate::util::time::now_ms;
        let temp = temp_data("logged");
        let running = service(&temp.config);
        let signed_in = running
            .set_up("owner@example.com", "a long password")
            .expect("set up");
        let setup = running
            .begin_two_factor_setup(&signed_in.user)
            .expect("begin");
        let secret = totp::base32_decode(&setup.secret_key);
        let code = format!("{:06}", totp::code_at(&secret, totp::step_at(now_ms())));
        running
            .enable_two_factor(signed_in.user.id, &code)
            .expect("enable");

        run(&temp.config, reset("Owner@Example.com ")).expect("reset");
        let newest = activity_log(&temp.config).remove(0);
        assert_eq!(newest.actor, Actor::Host);
        assert_eq!(newest.address, None, "no request, so no address");
        assert_eq!(
            newest.event,
            ActivityEvent::PasswordSet {
                email: "owner@example.com".to_string()
            },
            "the account's own email, not the text typed"
        );

        run(
            &temp.config,
            Command::Auth(AuthCommand::ResetTwoFactor {
                email: "owner@example.com".to_string(),
            }),
        )
        .expect("reset 2fa");
        assert_eq!(
            activity_log(&temp.config).remove(0).event,
            ActivityEvent::TwoFactorRemoved {
                email: "owner@example.com".to_string()
            }
        );

        run(&temp.config, Command::Auth(AuthCommand::Disable)).expect("disable");
        let newest = activity_log(&temp.config).remove(0);
        assert_eq!(newest.event, ActivityEvent::AccountsDisabled);
        assert_eq!(newest.actor, Actor::Host);
    }

    #[test]
    fn a_refused_recovery_command_is_not_logged() {
        let temp = temp_data("refused-not-logged");
        service(&temp.config)
            .set_up("owner@example.com", "a long password")
            .expect("set up");
        let before = activity_log(&temp.config).len();
        assert!(run(&temp.config, reset("nobody@example.com")).is_err());
        assert_eq!(activity_log(&temp.config).len(), before);
    }

    #[test]
    fn a_reset_is_refused_while_accounts_are_off() {
        let temp = temp_data("off");
        assert!(run(&temp.config, reset("owner@example.com")).is_err());
    }

    #[test]
    fn a_reset_signs_the_account_out_of_the_running_service() {
        let temp = temp_data("reset");
        let running = service(&temp.config);
        let signed_in = running
            .set_up("owner@example.com", "a long password")
            .expect("setup");

        run(&temp.config, reset("owner@example.com")).expect("reset");
        assert!(running
            .resolve(Some(&signed_in.token))
            .expect("resolve")
            .is_none());

        assert!(run(&temp.config, reset("nobody@example.com")).is_err());
    }

    #[test]
    fn disable_opens_the_install_the_running_service_serves() {
        let temp = temp_data("disable");
        let running = service(&temp.config);
        running
            .set_up("owner@example.com", "a long password")
            .expect("setup");

        run(&temp.config, Command::Auth(AuthCommand::Disable)).expect("disable");
        assert_eq!(running.mode().expect("mode"), AuthMode::Open);
        assert!(running.list_users().expect("users").is_empty());
    }

    #[test]
    fn reset_2fa_lets_the_account_back_in_with_its_password() {
        use crate::services::totp;
        use crate::util::time::now_ms;

        let temp = temp_data("reset-2fa");
        let running = service(&temp.config);
        let signed_in = running
            .set_up("owner@example.com", "a long password")
            .expect("setup");

        let command = || {
            Command::Auth(AuthCommand::ResetTwoFactor {
                email: "owner@example.com".to_string(),
            })
        };
        assert!(
            run(&temp.config, command()).is_err(),
            "nothing to reset yet"
        );

        let setup = running
            .begin_two_factor_setup(&signed_in.user)
            .expect("begin");
        let secret = totp::base32_decode(&setup.secret_key);
        let code = format!("{:06}", totp::code_at(&secret, totp::step_at(now_ms())));
        running
            .enable_two_factor(signed_in.user.id, &code)
            .expect("enable");

        run(&temp.config, command()).expect("reset");
        assert_eq!(
            running
                .two_factor_status(signed_in.user.id)
                .expect("status"),
            (false, 0)
        );
    }
}
