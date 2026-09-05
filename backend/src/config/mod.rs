//! Service configuration.
//!
//! Values are resolved with a builder in a fixed precedence order: compiled defaults, then environment
//! variables, then CLI flags. Only process level concerns live here. Anything a user can change while the
//! service is running belongs in the `settings` table instead, see `models::settings`.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use clap::Parser;

pub const DEFAULT_HOST: &str = "0.0.0.0";
pub const DEFAULT_PORT: u16 = 8080;
pub const DEFAULT_DATA_DIR: &str = "./data";
pub const DEFAULT_STATIC_DIR: &str = "../frontend/dist";
pub const DEFAULT_LOG_LEVEL: &str = "info";

/// Command line surface. Every flag is optional so that the environment can supply it instead.
#[derive(Debug, Parser)]
#[command(
    name = "on-air-record",
    version,
    about = "Audio broadcast and DVR service with a browser control room"
)]
pub struct CliArgs {
    /// Address to bind the HTTP server to
    #[arg(long)]
    pub host: Option<String>,

    /// HTTP port
    #[arg(long)]
    pub port: Option<u16>,

    /// Directory holding the SQLite database and the recorded segments
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    /// Directory holding the compiled web UI
    #[arg(long)]
    pub static_dir: Option<PathBuf>,

    /// Log level: error, warn, info, debug or trace
    #[arg(long)]
    pub log_level: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub static_dir: PathBuf,
    pub log_level: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            data_dir: PathBuf::from(DEFAULT_DATA_DIR),
            static_dir: PathBuf::from(DEFAULT_STATIC_DIR),
            log_level: DEFAULT_LOG_LEVEL.to_string(),
        }
    }
}

impl AppConfig {
    /// Resolve the effective configuration from defaults, environment, and CLI flags.
    pub fn resolve(args: CliArgs) -> Self {
        AppConfigBuilder::new()
            .with_environment()
            .with_cli(args)
            .build()
    }

    pub fn socket_addr(&self) -> SocketAddr {
        let ip: IpAddr = self
            .host
            .parse()
            .unwrap_or_else(|_| IpAddr::from([0, 0, 0, 0]));
        SocketAddr::new(ip, self.port)
    }

    /// Absolute path of the SQLite database file.
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("on-air-record.sqlite")
    }

    /// Root directory of the recorded segment files.
    pub fn recordings_dir(&self) -> PathBuf {
        self.data_dir.join("recordings")
    }

    /// Resolve a segment path stored relative to the data directory.
    pub fn resolve_data_path(&self, relative: &str) -> PathBuf {
        self.data_dir.join(relative)
    }

    /// Turn an absolute segment path back into the relative form stored in the database, so the data
    /// directory can be moved without invalidating the index.
    pub fn relativise_data_path(&self, absolute: &Path) -> String {
        absolute
            .strip_prefix(&self.data_dir)
            .unwrap_or(absolute)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Create the directories the service writes to. Called once during startup.
    pub fn ensure_directories(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.recordings_dir())?;
        Ok(())
    }
}

/// Builder that layers the configuration sources in precedence order.
#[derive(Debug)]
pub struct AppConfigBuilder {
    config: AppConfig,
}

impl AppConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: AppConfig::default(),
        }
    }

    /// Apply `OAR_*` environment variables. Unparseable values are ignored so a typo degrades to the
    /// default rather than preventing the service from starting.
    pub fn with_environment(mut self) -> Self {
        if let Ok(host) = std::env::var("OAR_HOST") {
            if !host.trim().is_empty() {
                self.config.host = host;
            }
        }
        if let Ok(port) = std::env::var("OAR_PORT") {
            if let Ok(parsed) = port.trim().parse::<u16>() {
                self.config.port = parsed;
            }
        }
        if let Ok(data_dir) = std::env::var("OAR_DATA_DIR") {
            if !data_dir.trim().is_empty() {
                self.config.data_dir = PathBuf::from(data_dir);
            }
        }
        if let Ok(static_dir) = std::env::var("OAR_STATIC_DIR") {
            if !static_dir.trim().is_empty() {
                self.config.static_dir = PathBuf::from(static_dir);
            }
        }
        if let Ok(log_level) = std::env::var("OAR_LOG_LEVEL") {
            if !log_level.trim().is_empty() {
                self.config.log_level = log_level;
            }
        }
        self
    }

    /// Apply CLI flags, which win over everything else.
    pub fn with_cli(mut self, args: CliArgs) -> Self {
        if let Some(host) = args.host {
            self.config.host = host;
        }
        if let Some(port) = args.port {
            self.config.port = port;
        }
        if let Some(data_dir) = args.data_dir {
            self.config.data_dir = data_dir;
        }
        if let Some(static_dir) = args.static_dir {
            self.config.static_dir = static_dir;
        }
        if let Some(log_level) = args.log_level {
            self.config.log_level = log_level;
        }
        self
    }

    pub fn build(self) -> AppConfig {
        self.config
    }
}

impl Default for AppConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_defaults() {
        let args = CliArgs {
            host: Some("127.0.0.1".to_string()),
            port: Some(9000),
            data_dir: None,
            static_dir: None,
            log_level: None,
        };
        let config = AppConfigBuilder::new().with_cli(args).build();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 9000);
        assert_eq!(config.log_level, DEFAULT_LOG_LEVEL);
    }

    #[test]
    fn database_path_sits_inside_data_dir() {
        let config = AppConfig {
            data_dir: PathBuf::from("/srv/oar"),
            ..AppConfig::default()
        };
        assert_eq!(
            config.database_path(),
            PathBuf::from("/srv/oar/on-air-record.sqlite")
        );
    }

    #[test]
    fn segment_paths_round_trip() {
        let config = AppConfig {
            data_dir: PathBuf::from("/srv/oar"),
            ..AppConfig::default()
        };
        let absolute = config.resolve_data_path("recordings/3/000012.pcm");
        assert_eq!(
            config.relativise_data_path(&absolute),
            "recordings/3/000012.pcm"
        );
    }

    #[test]
    fn invalid_host_falls_back_to_all_interfaces() {
        let config = AppConfig {
            host: "not-an-ip".to_string(),
            ..AppConfig::default()
        };
        assert_eq!(config.socket_addr().ip(), IpAddr::from([0, 0, 0, 0]));
    }
}
