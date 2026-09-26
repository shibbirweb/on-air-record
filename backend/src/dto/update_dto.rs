//! Update notice payloads.

use serde::Serialize;

use crate::models::update::{Channel, InstallKind, Release};
use crate::services::update_service::{UpdateStatus, REPOSITORY};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusResponse {
    pub current_version: String,
    pub channel: Channel,
    /// Whether the service checks by itself every few hours.
    pub automatic: bool,
    pub checked_at_ms: Option<i64>,
    pub error: Option<String>,
    /// The release to update to, when there is one: the newest on this channel.
    pub available: Option<ReleaseDto>,
    /// Every newer release, newest first, so What's new covers everything skipped.
    pub releases: Vec<ReleaseDto>,
    pub install: InstallDto,
    /// The page listing every release.
    pub releases_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseDto {
    /// Without the leading `v`, as the service reports its own version.
    pub version: String,
    pub tag: String,
    pub prerelease: bool,
    pub published_at_ms: Option<i64>,
    /// The release notes as written on the release, in Markdown.
    pub notes: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallDto {
    /// `installer`, `systemd` or `manual`.
    pub kind: &'static str,
    /// The installer folder, when there is one.
    pub dir: Option<String>,
    /// `linux`, `macos` or `windows`, which picks the installer and the commands to show.
    pub os: &'static str,
    /// The target triple this program was built for, which names its download.
    pub target: &'static str,
}

impl From<Release> for ReleaseDto {
    fn from(release: Release) -> Self {
        Self {
            version: release.tag.trim_start_matches('v').to_string(),
            tag: release.tag,
            prerelease: release.prerelease,
            published_at_ms: release.published_at_ms,
            notes: release.notes,
            url: release.url,
        }
    }
}

impl From<UpdateStatus> for UpdateStatusResponse {
    fn from(status: UpdateStatus) -> Self {
        let (kind, dir) = match status.install {
            InstallKind::Installer { dir } => ("installer", Some(dir)),
            InstallKind::Systemd => ("systemd", None),
            InstallKind::Manual => ("manual", None),
        };
        let releases: Vec<ReleaseDto> = status.newer.into_iter().map(Into::into).collect();
        Self {
            current_version: status.current,
            channel: status.channel,
            automatic: status.automatic,
            checked_at_ms: status.checked_at_ms,
            error: status.error,
            available: releases.first().map(|newest| ReleaseDto {
                version: newest.version.clone(),
                tag: newest.tag.clone(),
                prerelease: newest.prerelease,
                published_at_ms: newest.published_at_ms,
                notes: newest.notes.clone(),
                url: newest.url.clone(),
            }),
            releases,
            install: InstallDto {
                kind,
                dir,
                os: std::env::consts::OS,
                target: status.target,
            },
            releases_url: format!("https://github.com/{REPOSITORY}/releases"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(newer: Vec<Release>) -> UpdateStatus {
        UpdateStatus {
            current: "0.6.0".to_string(),
            channel: Channel::Stable,
            automatic: true,
            checked_at_ms: Some(5),
            error: None,
            newer,
            install: InstallKind::Installer {
                dir: "/home/me/on-air-record".to_string(),
            },
            target: "x86_64-unknown-linux-gnu",
        }
    }

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.to_string(),
            prerelease: false,
            draft: false,
            published_at_ms: Some(1),
            notes: "### Added".to_string(),
            url: "https://example.com".to_string(),
        }
    }

    #[test]
    fn the_newest_release_is_the_one_available() {
        let json = serde_json::to_value(UpdateStatusResponse::from(status(vec![
            release("v0.7.0"),
            release("v0.6.1"),
        ])))
        .expect("serialise");
        assert_eq!(json["currentVersion"], "0.6.0");
        assert_eq!(json["channel"], "stable");
        assert_eq!(json["available"]["version"], "0.7.0");
        assert_eq!(json["available"]["tag"], "v0.7.0");
        assert_eq!(json["releases"].as_array().map(Vec::len), Some(2));
        assert_eq!(json["install"]["kind"], "installer");
        assert_eq!(json["install"]["dir"], "/home/me/on-air-record");
        assert_eq!(json["install"]["target"], "x86_64-unknown-linux-gnu");
    }

    #[test]
    fn up_to_date_has_nothing_available() {
        let json = serde_json::to_value(UpdateStatusResponse::from(status(Vec::new())))
            .expect("serialise");
        assert!(json["available"].is_null());
        assert_eq!(json["releases"].as_array().map(Vec::len), Some(0));
    }
}
