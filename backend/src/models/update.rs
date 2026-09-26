//! Whether a newer release exists, and what it changes.
//!
//! Pure data and rules, kept apart from the HTTP call that fetches releases so the parts that decide
//! anything (version order, which channel sees which release, what counts as newer) are tested without
//! a network.

use std::cmp::Ordering;

/// Which releases this installation follows. Mirrors the installers' `--stable` and `--beta`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Stable releases only.
    Stable,
    /// The newest release of any kind, beta or stable.
    Beta,
}

/// A release version as the project numbers them: `0.6.0`, or `0.6.0-beta.2` before it.
///
/// Only the shapes `version.mjs` produces are understood. Anything else is not a version this code will
/// compare, which keeps a stray tag from being offered as an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The beta number, or `None` for a stable release.
    pub beta: Option<u64>,
}

impl Version {
    /// Parse `0.6.0`, `0.6.0-beta.2`, or either with a leading `v` as tags have.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let (core, beta) = match text.split_once('-') {
            Some((core, rest)) => (core, Some(rest.strip_prefix("beta.")?.parse().ok()?)),
            None => (text, None),
        };
        let mut parts = core.split('.');
        let version = Self {
            major: parts.next()?.parse().ok()?,
            minor: parts.next()?.parse().ok()?,
            patch: parts.next()?.parse().ok()?,
            beta,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(version)
    }

    pub fn is_beta(&self) -> bool {
        self.beta.is_some()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            // A beta comes before the stable release it previews, so no beta at all is the highest.
            .then_with(|| match (self.beta, other.beta) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(&right),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A published release, as much of it as the update notice needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The tag, like `v0.6.0`.
    pub tag: String,
    pub prerelease: bool,
    pub draft: bool,
    pub published_at_ms: Option<i64>,
    /// The release notes, as the Markdown written on the release.
    pub notes: String,
    /// The release page, for the full notes and the downloads.
    pub url: String,
}

/// The releases worth offering: published, on this channel, and newer than what is running. Newest first,
/// so the first is the one to install and the rest are what else changed on the way to it.
pub fn newer_releases(current: &Version, channel: Channel, releases: &[Release]) -> Vec<Release> {
    let mut offered: Vec<(Version, Release)> = releases
        .iter()
        .filter(|release| !release.draft)
        .filter(|release| channel == Channel::Beta || !release.prerelease)
        .filter_map(|release| Some((Version::parse(&release.tag)?, release.clone())))
        // A tag and its pre-release flag must agree, which the release workflow enforces; a beta version
        // that somehow is not flagged still stays off the stable channel.
        .filter(|(version, _)| channel == Channel::Beta || !version.is_beta())
        .filter(|(version, _)| version > current)
        .collect();
    offered.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    offered.into_iter().map(|(_, release)| release).collect()
}

/// Which channel to follow when nothing says otherwise: a beta build keeps following betas, which is what
/// its installer was told, and anything else follows stable releases.
pub fn default_channel(current: &Version) -> Channel {
    if current.is_beta() {
        Channel::Beta
    } else {
        Channel::Stable
    }
}

/// Read `OAR_CHANNEL` from the installer's `config` file, a list of `KEY=value` lines.
pub fn channel_from_config(text: &str) -> Option<Channel> {
    text.lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("OAR_CHANNEL="))
        .and_then(|value| match value.trim().trim_matches('"') {
            "beta" => Some(Channel::Beta),
            "stable" => Some(Channel::Stable),
            _ => None,
        })
}

/// How this copy was installed, which decides how it is updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// A folder made by `install.sh` or `install.ps1`: run the installer again with `--update`.
    Installer { dir: String },
    /// Run by systemd, as the setup guide describes: replace the program and restart the unit.
    Systemd,
    /// Anything else, such as a program started by hand: download the new one from the release page.
    Manual,
}

/// Work out the install kind from what the process can see. `INVOCATION_ID` is set by systemd for every
/// unit it starts; the installers write a start script beside the program.
pub fn detect_install(
    invocation_id: Option<&str>,
    program_dir: Option<&std::path::Path>,
) -> InstallKind {
    if invocation_id.is_some_and(|id| !id.is_empty()) {
        return InstallKind::Systemd;
    }
    match program_dir {
        Some(dir) if dir.join("start.sh").is_file() || dir.join("start.cmd").is_file() => {
            InstallKind::Installer {
                dir: dir.display().to_string(),
            }
        }
        _ => InstallKind::Manual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool) -> Release {
        Release {
            tag: tag.to_string(),
            prerelease,
            draft: false,
            published_at_ms: Some(0),
            notes: format!("notes for {tag}"),
            url: format!("https://example.com/{tag}"),
        }
    }

    fn version(text: &str) -> Version {
        Version::parse(text).expect("a version")
    }

    #[test]
    fn versions_parse_in_the_shapes_the_project_uses() {
        assert_eq!(
            Version::parse("0.6.0"),
            Some(Version {
                major: 0,
                minor: 6,
                patch: 0,
                beta: None
            })
        );
        assert_eq!(version("v0.6.0-beta.2").beta, Some(2));
        assert_eq!(version(" v1.2.3 "), version("1.2.3"));
    }

    #[test]
    fn anything_else_is_not_a_version() {
        for text in [
            "",
            "v",
            "0.6",
            "0.6.0.1",
            "0.6.0-rc.1",
            "0.6.0-beta",
            "latest",
            "v0.x.0",
        ] {
            assert_eq!(Version::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_beta_comes_before_its_release_and_after_the_one_before() {
        let mut ordered = [
            version("0.6.0"),
            version("0.5.1"),
            version("0.6.0-beta.2"),
            version("0.6.0-beta.10"),
            version("0.6.0-beta.1"),
            version("0.10.0"),
        ];
        ordered.sort();
        let tags: Vec<String> = ordered
            .iter()
            .map(|v| match v.beta {
                Some(beta) => format!("{}.{}.{}-beta.{beta}", v.major, v.minor, v.patch),
                None => format!("{}.{}.{}", v.major, v.minor, v.patch),
            })
            .collect();
        assert_eq!(
            tags,
            [
                "0.5.1",
                "0.6.0-beta.1",
                "0.6.0-beta.2",
                "0.6.0-beta.10",
                "0.6.0",
                "0.10.0"
            ]
        );
    }

    #[test]
    fn stable_sees_only_newer_stable_releases_newest_first() {
        let releases = [
            release("v0.7.0-beta.1", true),
            release("v0.6.1", false),
            release("v0.6.0", false),
            release("v0.5.0", false),
            release("v0.7.0", false),
        ];
        let offered = newer_releases(&version("0.6.0"), Channel::Stable, &releases);
        let tags: Vec<&str> = offered.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags, ["v0.7.0", "v0.6.1"]);
    }

    #[test]
    fn beta_sees_betas_and_stable_releases() {
        let releases = [release("v0.7.0-beta.1", true), release("v0.6.1", false)];
        let offered = newer_releases(&version("0.6.0"), Channel::Beta, &releases);
        let tags: Vec<&str> = offered.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags, ["v0.7.0-beta.1", "v0.6.1"]);
    }

    #[test]
    fn a_beta_build_is_offered_the_release_it_previews() {
        let releases = [release("v0.6.0", false), release("v0.6.0-beta.1", true)];
        let offered = newer_releases(&version("0.6.0-beta.1"), Channel::Beta, &releases);
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].tag, "v0.6.0");
    }

    #[test]
    fn drafts_odd_tags_and_mislabelled_betas_are_never_offered() {
        let mut draft = release("v0.9.0", false);
        draft.draft = true;
        let releases = [
            draft,
            release("nightly", false),
            // A beta version published without the pre-release flag: still not for stable.
            release("v0.8.0-beta.1", false),
        ];
        assert!(newer_releases(&version("0.6.0"), Channel::Stable, &releases).is_empty());
    }

    #[test]
    fn nothing_is_offered_when_up_to_date() {
        let releases = [release("v0.6.0", false), release("v0.5.0", false)];
        assert!(newer_releases(&version("0.6.0"), Channel::Stable, &releases).is_empty());
    }

    #[test]
    fn a_beta_build_follows_betas_unless_told_otherwise() {
        assert_eq!(default_channel(&version("0.6.0-beta.1")), Channel::Beta);
        assert_eq!(default_channel(&version("0.6.0")), Channel::Stable);
    }

    #[test]
    fn the_installer_config_names_the_channel() {
        let config = "OAR_PORT=8080\nOAR_HOST=0.0.0.0\nOAR_CHANNEL=beta\n";
        assert_eq!(channel_from_config(config), Some(Channel::Beta));
        assert_eq!(
            channel_from_config("OAR_CHANNEL=stable"),
            Some(Channel::Stable)
        );
        // The last line wins, as it would when the shell sources the file.
        assert_eq!(
            channel_from_config("OAR_CHANNEL=beta\nOAR_CHANNEL=stable"),
            Some(Channel::Stable)
        );
        assert_eq!(channel_from_config("OAR_PORT=8080"), None);
        assert_eq!(channel_from_config("OAR_CHANNEL=nightly"), None);
    }

    #[test]
    fn the_install_kind_is_read_from_what_the_process_can_see() {
        let dir = std::env::temp_dir().join(format!("oar-install-kind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");

        assert_eq!(
            detect_install(Some("abc123"), Some(&dir)),
            InstallKind::Systemd
        );
        assert_eq!(detect_install(None, Some(&dir)), InstallKind::Manual);
        assert_eq!(detect_install(None, None), InstallKind::Manual);

        std::fs::write(dir.join("start.sh"), "#!/bin/sh\n").expect("start script");
        assert_eq!(
            detect_install(None, Some(&dir)),
            InstallKind::Installer {
                dir: dir.display().to_string()
            }
        );
        // systemd wins: a unit may well run a program that sits in an installer folder.
        assert_eq!(
            detect_install(Some("abc123"), Some(&dir)),
            InstallKind::Systemd
        );
        assert_eq!(
            detect_install(Some(""), Some(&dir)).clone(),
            detect_install(None, Some(&dir))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
