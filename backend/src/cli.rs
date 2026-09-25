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
use crate::repositories::AuthRepository;
use crate::services::AuthService;

pub fn run(config: &AppConfig, command: Command) -> AppResult<()> {
    let database = Arc::new(Database::open(&config.database_path())?);
    let auth = AuthService::new(Arc::new(AuthRepository::new(database)));

    match command {
        Command::Auth(AuthCommand::ResetPassword { email }) => {
            let password = auth.reset_password_by_email(&email)?;
            println!("New password for {}: {password}", email.trim());
            println!("Every session of that account has been signed out.");
            println!("Sign in with it, then change it under your account menu.");
        }
        Command::Auth(AuthCommand::Disable) => {
            auth.disable_accounts()?;
            println!("Accounts are switched off and every account has been deleted.");
            println!("Anyone who can reach the page can now use it without signing in.");
            println!("Switch accounts back on from the settings page.");
        }
    }

    Ok(())
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
}
