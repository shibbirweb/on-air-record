//! Asks GitHub every few hours whether a newer release exists, and keeps the answer for the UI.
//!
//! Notify only: this never downloads or installs anything. It reads the public releases list, keeps the
//! releases newer than the running version on this installation's channel, and the UI shows an admin what
//! changed and how to update for the way this copy was installed.
//!
//! It is the only request the service makes to the internet, so it honours the `check_for_updates`
//! setting, waits a minute after start so booting never depends on the network, and treats every failure
//! as "try again later": an offline recorder is a normal recorder.

use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::models::update::{
    channel_from_config, default_channel, detect_install, newer_releases, Channel, InstallKind,
    Release, Version,
};
use crate::services::SettingsService;
use crate::util::time::now_ms;

/// Where releases are published.
pub const REPOSITORY: &str = "shibbirweb/on-air-record";

/// Often enough that a release is noticed the same day, rarely enough to stay far inside GitHub's limit
/// of 60 unauthenticated requests an hour, which every recorder behind one address shares.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Booting must never wait on the network, and a service that restarts in a loop should not hammer
/// GitHub on every attempt.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Thirty releases of notes is a few hundred kilobytes. Anything far larger is not the releases list.
const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

/// Everything the UI needs to say whether an update exists and how to get it.
#[derive(Debug, Clone)]
pub struct UpdateStatus {
    pub current: String,
    pub channel: Channel,
    /// Whether automatic checks are on. A check can still be asked for by hand when they are off.
    pub automatic: bool,
    pub checked_at_ms: Option<i64>,
    /// Why the last check failed, if it did. The previous answer is kept alongside it.
    pub error: Option<String>,
    /// Newer releases on this channel, newest first. Empty when up to date or never checked.
    pub newer: Vec<Release>,
    pub install: InstallKind,
    /// The target triple this program was built for, which names its download.
    pub target: &'static str,
}

#[derive(Default)]
struct CheckState {
    checked_at_ms: Option<i64>,
    error: Option<String>,
    newer: Vec<Release>,
}

pub struct UpdateService {
    settings: Arc<SettingsService>,
    current: Option<Version>,
    channel: Channel,
    install: InstallKind,
    state: Mutex<CheckState>,
    /// One check at a time, so a Check now pressed during the scheduled one waits for it instead of
    /// asking GitHub twice.
    checking: tokio::sync::Mutex<()>,
}

impl UpdateService {
    pub fn new(settings: Arc<SettingsService>) -> Self {
        let current = Version::parse(env!("CARGO_PKG_VERSION"));
        let program_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(PathBuf::from));
        let install = detect_install(
            std::env::var("INVOCATION_ID").ok().as_deref(),
            program_dir.as_deref(),
        );
        Self {
            settings,
            channel: detect_channel(current.as_ref(), program_dir.as_deref()),
            current,
            install,
            state: Mutex::new(CheckState::default()),
            checking: tokio::sync::Mutex::new(()),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        let (checked_at_ms, error, newer) = match self.state.lock() {
            Ok(state) => (
                state.checked_at_ms,
                state.error.clone(),
                state.newer.clone(),
            ),
            Err(_) => (None, None, Vec::new()),
        };
        UpdateStatus {
            current: env!("CARGO_PKG_VERSION").to_string(),
            channel: self.channel,
            automatic: self.settings.current().check_for_updates,
            checked_at_ms,
            error,
            newer,
            install: self.install.clone(),
            target: option_env!("OAR_TARGET").unwrap_or("unknown"),
        }
    }

    /// Ask GitHub now and remember the answer. Failures are recorded, not returned: the caller always
    /// gets a status to show.
    pub async fn check_now(&self) -> UpdateStatus {
        let _one_at_a_time = self.checking.lock().await;
        let outcome = tokio::task::spawn_blocking(fetch_releases)
            .await
            .unwrap_or_else(|error| Err(format!("the update check stopped unexpectedly: {error}")));

        if let Ok(mut state) = self.state.lock() {
            state.checked_at_ms = Some(now_ms());
            match outcome {
                Ok(releases) => {
                    state.newer = match &self.current {
                        Some(current) => newer_releases(current, self.channel, &releases),
                        None => Vec::new(),
                    };
                    state.error = None;
                    if let Some(newest) = state.newer.first() {
                        tracing::info!(release = %newest.tag, "a newer release is available");
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "update check failed");
                    state.error = Some(error);
                }
            }
        }
        self.status()
    }

    /// Check a minute after start and then every few hours, while the setting allows it.
    pub async fn run(self: Arc<Self>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut wait = FIRST_CHECK_DELAY;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = shutdown.changed() => break,
            }
            if self.settings.current().check_for_updates {
                self.check_now().await;
            }
            wait = CHECK_INTERVAL;
        }
    }
}

/// The channel this copy follows: `OAR_CHANNEL` in the environment, then the installer's config file
/// beside the program (the start script does not export it), then whatever the running version implies.
fn detect_channel(current: Option<&Version>, program_dir: Option<&std::path::Path>) -> Channel {
    if let Ok(value) = std::env::var("OAR_CHANNEL") {
        if let Some(channel) = channel_from_config(&format!("OAR_CHANNEL={value}")) {
            return channel;
        }
    }
    if let Some(channel) = program_dir
        .and_then(|dir| std::fs::read_to_string(dir.join("config")).ok())
        .and_then(|text| channel_from_config(&text))
    {
        return channel;
    }
    current.map(default_channel).unwrap_or(Channel::Stable)
}

/// Read the public releases list. Blocking, so it runs on a blocking thread.
fn fetch_releases() -> Result<Vec<Release>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .https_only(true)
        .build()
        .into();
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=30");
    let response = agent
        .get(&url)
        // GitHub refuses requests without a user agent, and naming the version helps anyone reading
        // their logs.
        .header(
            "User-Agent",
            concat!("on-air-record/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(describe_error)?;

    let mut body = String::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_RESPONSE_BYTES)
        .read_to_string(&mut body)
        .map_err(|error| format!("could not read the releases from GitHub: {error}"))?;
    parse_releases(&body)
}

fn describe_error(error: ureq::Error) -> String {
    match error {
        // GitHub answers a rate limited client with 403 or 429. Every recorder behind one address shares
        // the allowance, so say plainly that it is temporary.
        ureq::Error::StatusCode(403 | 429) => {
            "GitHub is limiting requests from this network for now; it will try again later"
                .to_string()
        }
        ureq::Error::StatusCode(code) => format!("GitHub answered with status {code}"),
        other => format!("could not reach GitHub: {other}"),
    }
}

#[derive(serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    published_at: Option<String>,
    body: Option<String>,
    html_url: String,
}

/// Turn GitHub's releases list into releases. Unknown fields are ignored, missing optional ones default.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, String> {
    let releases: Vec<GithubRelease> = serde_json::from_str(json).map_err(|error| {
        format!("GitHub sent a releases list this version cannot read: {error}")
    })?;
    Ok(releases
        .into_iter()
        .map(|release| Release {
            tag: release.tag_name,
            prerelease: release.prerelease,
            draft: release.draft,
            published_at_ms: release
                .published_at
                .as_deref()
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .map(|instant| instant.timestamp_millis()),
            notes: release.body.unwrap_or_default(),
            url: release.html_url,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
        {
            "tag_name": "v0.7.0-beta.1",
            "prerelease": true,
            "draft": false,
            "published_at": "2026-10-01T12:00:00Z",
            "body": "\n### Added\n\n- **Something new.** (OAR-90)\n",
            "html_url": "https://github.com/shibbirweb/on-air-record/releases/tag/v0.7.0-beta.1",
            "assets": [],
            "author": { "login": "someone" }
        },
        {
            "tag_name": "v0.6.0",
            "published_at": null,
            "body": null,
            "html_url": "https://github.com/shibbirweb/on-air-record/releases/tag/v0.6.0"
        }
    ]"#;

    #[test]
    fn github_releases_are_read_with_only_the_fields_that_matter() {
        let releases = parse_releases(SAMPLE).expect("parse");
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag, "v0.7.0-beta.1");
        assert!(releases[0].prerelease);
        assert_eq!(releases[0].published_at_ms, Some(1_790_856_000_000));
        assert!(releases[0].notes.contains("Something new"));
        // Missing and null fields fall back rather than failing the whole list.
        assert!(!releases[1].prerelease);
        assert_eq!(releases[1].published_at_ms, None);
        assert_eq!(releases[1].notes, "");
    }

    #[test]
    fn something_that_is_not_a_releases_list_is_an_error_to_show() {
        assert!(parse_releases(r#"{"message": "Not Found"}"#).is_err());
        assert!(parse_releases("<html>").is_err());
    }

    #[test]
    fn a_rate_limit_reads_as_temporary() {
        assert!(describe_error(ureq::Error::StatusCode(403)).contains("try again later"));
        assert!(describe_error(ureq::Error::StatusCode(429)).contains("try again later"));
        assert!(describe_error(ureq::Error::StatusCode(502)).contains("502"));
    }

    #[test]
    fn the_channel_comes_from_the_installer_config_before_the_version() {
        let dir = std::env::temp_dir().join(format!("oar-update-channel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let stable = Version::parse("0.6.0").expect("version");
        let beta = Version::parse("0.6.0-beta.1").expect("version");

        assert_eq!(detect_channel(Some(&stable), Some(&dir)), Channel::Stable);
        assert_eq!(detect_channel(Some(&beta), Some(&dir)), Channel::Beta);

        std::fs::write(dir.join("config"), "OAR_PORT=8080\nOAR_CHANNEL=beta\n").expect("config");
        assert_eq!(detect_channel(Some(&stable), Some(&dir)), Channel::Beta);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
