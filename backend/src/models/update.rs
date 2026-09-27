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
            Some((core, rest)) => (core, Some(number(rest.strip_prefix("beta.")?)?)),
            None => (text, None),
        };
        let mut parts = core.split('.');
        let version = Self {
            major: number(parts.next()?)?,
            minor: number(parts.next()?)?,
            patch: number(parts.next()?)?,
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

/// One numeric part of a version: digits only. `u64::from_str` alone also takes a leading `+`, which
/// would let a tag like `v0.+7.0` through as 0.7.0.
fn number(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
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
    /// The published container image: pull the new image and recreate the container. Replacing the
    /// program inside it would be lost the next time the container is recreated.
    Docker,
    /// Anything else, such as a program started by hand: download the new one from the release page.
    Manual,
}

/// Work out the install kind from what the process can see. The image sets `OAR_CONTAINER` rather than
/// this guessing from `/.dockerenv`, which Podman and other runtimes do not create. `INVOCATION_ID` is
/// set by systemd for every unit it starts; the installers write a start script beside the program.
pub fn detect_install(
    container: Option<&str>,
    invocation_id: Option<&str>,
    program_dir: Option<&std::path::Path>,
) -> InstallKind {
    // First, because nothing on the host reaches inside a container: a unit that runs `docker run` gives
    // the program no INVOCATION_ID, and the image has no installer folder, but saying so costs nothing.
    if container.is_some_and(|name| !name.is_empty()) {
        return InstallKind::Docker;
    }
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
            // Found by `only_the_published_shape_is_a_version`: `u64::from_str` takes a leading plus.
            "0.+7.0",
            "0.0.0-beta.+0",
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
            detect_install(None, Some("abc123"), Some(&dir)),
            InstallKind::Systemd
        );
        assert_eq!(detect_install(None, None, Some(&dir)), InstallKind::Manual);
        assert_eq!(detect_install(None, None, None), InstallKind::Manual);

        std::fs::write(dir.join("start.sh"), "#!/bin/sh\n").expect("start script");
        assert_eq!(
            detect_install(None, None, Some(&dir)),
            InstallKind::Installer {
                dir: dir.display().to_string()
            }
        );
        // systemd wins: a unit may well run a program that sits in an installer folder.
        assert_eq!(
            detect_install(None, Some("abc123"), Some(&dir)),
            InstallKind::Systemd
        );
        assert_eq!(
            detect_install(None, Some(""), Some(&dir)).clone(),
            detect_install(None, None, Some(&dir))
        );

        // The image says so, and that beats everything else the process can see.
        assert_eq!(
            detect_install(Some("docker"), Some("abc123"), Some(&dir)),
            InstallKind::Docker
        );
        assert_eq!(
            detect_install(Some(""), None, Some(&dir)),
            detect_install(None, None, Some(&dir))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        /// Versions from a small range, so equal parts and equal versions come up often enough for the
        /// ordering properties to test ties, not just different majors.
        fn close_version() -> impl Strategy<Value = Version> {
            (0u64..3, 0u64..3, 0u64..3, proptest::option::of(0u64..3)).prop_map(
                |(major, minor, patch, beta)| Version {
                    major,
                    minor,
                    patch,
                    beta,
                },
            )
        }

        fn spelled(version: &Version) -> String {
            let core = format!("{}.{}.{}", version.major, version.minor, version.patch);
            match version.beta {
                Some(beta) => format!("{core}-beta.{beta}"),
                None => core,
            }
        }

        fn release_of(tag: String, prerelease: bool, draft: bool) -> Release {
            Release {
                url: format!("https://example.com/{tag}"),
                tag,
                prerelease,
                draft,
                published_at_ms: None,
                notes: String::new(),
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// Any version, spelled the way `version.mjs` writes it, as a tag, or with stray whitespace,
            /// parses back to itself, including numbers far past anything the project will reach.
            #[test]
            fn every_version_parses_back_from_its_spelling(
                major in any::<u64>(),
                minor in any::<u64>(),
                patch in any::<u64>(),
                beta in proptest::option::of(any::<u64>()),
            ) {
                let version = Version { major, minor, patch, beta };
                let text = spelled(&version);
                prop_assert_eq!(Version::parse(&text), Some(version));
                prop_assert_eq!(Version::parse(&format!("v{text}")), Some(version));
                prop_assert_eq!(Version::parse(&format!(" v{text}\n")), Some(version));
            }

            /// A tag comes from GitHub, so parsing must answer for any text, and whatever it accepts has
            /// the only shape the project publishes: digits, dots, and an optional `-beta.N`. Near misses
            /// are generated on purpose, since random text almost never looks like a version at all.
            #[test]
            fn only_the_published_shape_is_a_version(text in prop_oneof![
                ".{0,24}",
                "[ v]{0,2}[0-9+_ -]{1,3}\\.[0-9+_ -]{1,3}\\.[0-9+_ -]{1,3}(-beta\\.[0-9+_ -]{1,3})?",
                "v?[0-9+]{1,3}\\.[0-9+]{1,3}\\.[0-9+]{1,3}(-beta\\.[0-9+]{1,3})?",
            ]) {
                if Version::parse(&text).is_some() {
                    let bare = text.trim();
                    let bare = bare.strip_prefix('v').unwrap_or(bare);
                    let (core, beta) = match bare.split_once("-beta.") {
                        Some((core, beta)) => (core, Some(beta)),
                        None => (bare, None),
                    };
                    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
                    let parts: Vec<&str> = core.split('.').collect();
                    prop_assert!(parts.len() == 3 && parts.iter().all(|part| digits(part)), "accepted {:?}", text);
                    prop_assert!(beta.is_none_or(digits), "accepted {:?}", text);
                }
            }

            /// The order is a total order that agrees with equality: exactly one of less, equal or greater,
            /// the reverse when the sides swap, and transitive. `newer_releases` sorts by it and compares
            /// with it, so a broken order could offer an older release, or none, depending on list order.
            #[test]
            fn versions_are_totally_ordered(a in close_version(), b in close_version(), c in close_version()) {
                prop_assert_eq!(a.cmp(&b) == Ordering::Equal, a == b);
                prop_assert_eq!(a.cmp(&b), b.cmp(&a).reverse());
                if a <= b && b <= c {
                    prop_assert!(a <= c);
                }
            }

            /// Every beta comes after everything with a lower version number and before the release it
            /// previews, and betas of one release follow their own numbers.
            #[test]
            fn a_beta_sits_between_the_last_release_and_its_own(
                release in close_version().prop_map(|version| Version { beta: None, ..version }),
                beta in any::<u64>(),
                other in any::<u64>(),
            ) {
                let preview = Version { beta: Some(beta), ..release };
                prop_assert!(preview < release);
                let earlier = Version { patch: release.patch.wrapping_sub(1), beta: None, ..release };
                if release.patch > 0 {
                    prop_assert!(earlier < preview);
                }
                let sibling = Version { beta: Some(other), ..release };
                prop_assert_eq!(preview.cmp(&sibling), beta.cmp(&other));
            }

            /// What is offered is exactly the published releases newer than the running version that the
            /// channel may see, newest first: nothing older, no drafts, no betas on stable however they
            /// are flagged, and nothing that qualifies left out.
            #[test]
            fn only_newer_releases_on_the_channel_are_offered(
                current in close_version(),
                beta_channel in any::<bool>(),
                listed in proptest::collection::vec((close_version(), any::<bool>(), proptest::bool::weighted(0.2), any::<bool>()), 0..12),
            ) {
                let channel = if beta_channel { Channel::Beta } else { Channel::Stable };
                let releases: Vec<Release> = listed
                    .iter()
                    .map(|(version, prerelease, draft, as_tag)| {
                        let tag = if *as_tag { format!("v{}", spelled(version)) } else { spelled(version) };
                        release_of(tag, *prerelease, *draft)
                    })
                    .collect();
                let offered = newer_releases(&current, channel, &releases);

                let versions: Vec<Version> = offered
                    .iter()
                    .map(|release| Version::parse(&release.tag).expect("only parsed tags are offered"))
                    .collect();
                for (release, version) in offered.iter().zip(&versions) {
                    prop_assert!(*version > current);
                    prop_assert!(!release.draft);
                    if channel == Channel::Stable {
                        prop_assert!(!release.prerelease && !version.is_beta());
                    }
                }
                prop_assert!(versions.windows(2).all(|pair| pair[0] >= pair[1]));

                let qualifying = listed
                    .iter()
                    .filter(|(version, prerelease, draft, _)| {
                        !draft && *version > current
                            && (channel == Channel::Beta || (!prerelease && !version.is_beta()))
                    })
                    .count();
                prop_assert_eq!(offered.len(), qualifying);
            }

            /// Reading the installer's config never panics on any file contents, and a channel is only
            /// ever named by one of the two words.
            #[test]
            fn any_config_names_a_channel_or_none(text in "(.{0,20}\n){0,4}(OAR_CHANNEL=.{0,10})?") {
                let named = channel_from_config(&text);
                if named.is_some() {
                    prop_assert!(text.contains("beta") || text.contains("stable"));
                }
            }
        }
    }
}
