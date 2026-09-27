//! Why a container cannot reach the sound cards, in words that name the fix.
//!
//! Inside Docker every access failure looks the same from the web page: no microphones. The kernel does
//! say which of three things is wrong, though, and the setup guide is built around the same three: the
//! device files are not in the container, the container is not allowed to open devices (a `/dev/snd`
//! mounted as a volume), or the program is not in the group that owns them. This looks, so the log and
//! the recorder panel can say which one and what to change, instead of cpal's "no usable input config".
//!
//! It opens a *control* device, never a PCM one: any number of programs may hold a control device open,
//! so asking never takes the microphone from anybody. Gathering is std only, Linux only, and cheap enough
//! to run at boot and after a failed start. The decision is [`diagnose`], a pure function, so every branch
//! is tested without a sound card or a container.

use std::collections::BTreeSet;
use std::path::Path;

/// The image sets this; see the Dockerfile. Advice about compose files means nothing anywhere else.
pub fn in_container() -> bool {
    std::env::var("OAR_CONTAINER").is_ok_and(|value| !value.is_empty())
}

/// What opening a control device told us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenOutcome {
    Opened,
    /// `EPERM`: the file permissions allowed it, but the container may not use the device at all.
    NotPermitted,
    /// `EACCES`: the file permissions refused it.
    PermissionDenied,
    /// Anything else, such as no driver behind the device. Not an access problem, so no advice.
    Other,
}

/// Everything [`diagnose`] needs, gathered by [`probe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessProbe {
    pub devices_present: bool,
    /// Whether any card's control device exists, which is one per sound card.
    pub has_card: bool,
    /// The groups owning the files in `/dev/snd`.
    pub owner_groups: BTreeSet<u32>,
    /// The effective group and the supplementary groups this process runs with.
    pub process_groups: BTreeSet<u32>,
    pub is_root: bool,
    /// `None` when there was no control device to try.
    pub open: Option<OpenOutcome>,
}

/// One of the reasons a container cannot reach the sound cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessProblem {
    /// `/dev/snd` does not exist in the container.
    NotPassedIn,
    /// `/dev/snd` exists but holds no sound card, so the host has none to give.
    NoSoundCard,
    /// The files are there but the container may not open devices: mounted as a volume.
    MountedAsVolume,
    /// The program lacks the group that owns the files. These are the missing numbers.
    WrongGroup { missing: Vec<u32> },
    /// Refused although the groups match, which leaves a security module such as SELinux.
    BlockedDespiteGroup,
}

impl AccessProblem {
    /// What to change, for somebody reading the log or the recorder panel. Names the compose and the
    /// `docker run` spelling both, since nothing here knows which was used.
    pub fn advice(&self) -> String {
        match self {
            Self::NotPassedIn => "This container has no sound devices: /dev/snd was not passed in. Add it under \
                 devices: in compose.yaml, or --device /dev/snd to docker run, then recreate the container."
                .to_string(),
            Self::NoSoundCard => "/dev/snd reached the container but holds no sound card, so the host has not \
                 found a microphone. Check cat /proc/asound/cards on the host; a microphone plugged in after \
                 the container started needs docker compose restart."
                .to_string(),
            Self::MountedAsVolume => "The container is not allowed to open /dev/snd, which happens when it is \
                 mounted as a volume. Remove it from volumes: and pass it under devices: in compose.yaml, or \
                 with --device /dev/snd, then recreate the container."
                .to_string(),
            Self::WrongGroup { missing } => {
                let numbers = missing
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                let flags = missing
                    .iter()
                    .map(|gid| format!("--group-add {gid}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let setting = match missing.as_slice() {
                    [only] => format!("set AUDIO_GID={only} in .env"),
                    _ => format!("list {numbers} under group_add: in compose.yaml"),
                };
                format!(
                    "The container is not in the group that owns /dev/snd (group {numbers}). {} and run \
                     docker compose up -d, or with docker run add {flags}.",
                    capitalise(&setting)
                )
            }
            Self::BlockedDespiteGroup => "Opening /dev/snd was refused although the container has the right \
                 group, which points at SELinux or AppArmor. On SELinux, sudo ausearch -m avc -ts recent on \
                 the host shows the denial; the setup guide's SELinux notes have a lead."
                .to_string(),
        }
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Which problem, if any. Only an access failure the kernel actually reported produces advice: an open
/// that worked, or failed for some other reason, says nothing, so a working setup is never nagged.
pub fn diagnose(probe: &AccessProbe) -> Option<AccessProblem> {
    if !probe.devices_present {
        return Some(AccessProblem::NotPassedIn);
    }
    if !probe.has_card {
        return Some(AccessProblem::NoSoundCard);
    }
    match probe.open? {
        OpenOutcome::Opened | OpenOutcome::Other => None,
        OpenOutcome::NotPermitted => Some(AccessProblem::MountedAsVolume),
        OpenOutcome::PermissionDenied => {
            let missing: Vec<u32> = probe
                .owner_groups
                .difference(&probe.process_groups)
                .copied()
                .collect();
            if probe.is_root || missing.is_empty() {
                Some(AccessProblem::BlockedDespiteGroup)
            } else {
                Some(AccessProblem::WrongGroup { missing })
            }
        }
    }
}

/// The effective uid and every group from `/proc/self/status`, which is the kernel's own view of this
/// process, so it cannot disagree with what an open will be judged against.
pub fn parse_process_status(status: &str) -> (Option<u32>, BTreeSet<u32>) {
    // Uid: and Gid: list real, effective, saved and filesystem ids; the second is the one that counts.
    let second = |prefix: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .and_then(|rest| rest.split_whitespace().nth(1))
            .and_then(|id| id.parse::<u32>().ok())
    };
    let mut groups: BTreeSet<u32> = status
        .lines()
        .find_map(|line| line.strip_prefix("Groups:"))
        .map(|rest| {
            rest.split_whitespace()
                .filter_map(|id| id.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    if let Some(gid) = second("Gid:") {
        groups.insert(gid);
    }
    (second("Uid:"), groups)
}

/// Look at `/dev/snd` and try its first control device. `root` is a parameter so tests can point it at a
/// temporary folder.
#[cfg(target_os = "linux")]
pub fn probe(root: &Path) -> AccessProbe {
    use std::os::unix::fs::MetadataExt;

    // Linux's numbers, written out rather than taking a libc dependency for two constants. std maps both to
    // ErrorKind::PermissionDenied, which is exactly the distinction this needs to keep.
    const EPERM: i32 = 1;
    const EACCES: i32 = 13;

    let (uid, process_groups) = std::fs::read_to_string("/proc/self/status")
        .map(|status| parse_process_status(&status))
        .unwrap_or_default();

    let Ok(entries) = std::fs::read_dir(root) else {
        return AccessProbe {
            devices_present: false,
            has_card: false,
            owner_groups: BTreeSet::new(),
            process_groups,
            is_root: uid == Some(0),
            open: None,
        };
    };

    let mut owner_groups = BTreeSet::new();
    let mut control = None;
    for entry in entries.flatten() {
        if let Ok(metadata) = entry.metadata() {
            owner_groups.insert(metadata.gid());
        }
        let is_control = entry.file_name().to_string_lossy().starts_with("controlC");
        if is_control && control.is_none() {
            control = Some(entry.path());
        }
    }

    let open =
        control.as_ref().map(
            |path| match std::fs::OpenOptions::new().read(true).open(path) {
                Ok(_) => OpenOutcome::Opened,
                Err(error) => match error.raw_os_error() {
                    Some(EPERM) => OpenOutcome::NotPermitted,
                    Some(EACCES) => OpenOutcome::PermissionDenied,
                    _ => OpenOutcome::Other,
                },
            },
        );

    AccessProbe {
        devices_present: true,
        has_card: control.is_some(),
        owner_groups,
        process_groups,
        is_root: uid == Some(0),
        open,
    }
}

/// Only Linux containers pass sound cards through `/dev/snd`, so elsewhere there is nothing to say.
#[cfg(not(target_os = "linux"))]
pub fn probe(_root: &Path) -> AccessProbe {
    AccessProbe {
        devices_present: true,
        has_card: true,
        owner_groups: BTreeSet::new(),
        process_groups: BTreeSet::new(),
        is_root: false,
        open: None,
    }
}

/// The problem with this container's sound devices, or `None` when there is none or this is not a
/// container.
pub fn check_container() -> Option<AccessProblem> {
    if !in_container() {
        return None;
    }
    diagnose(&probe(Path::new("/dev/snd")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn working() -> AccessProbe {
        AccessProbe {
            devices_present: true,
            has_card: true,
            owner_groups: BTreeSet::from([29]),
            process_groups: BTreeSet::from([29, 999]),
            is_root: false,
            open: Some(OpenOutcome::Opened),
        }
    }

    #[test]
    fn a_working_setup_gets_no_advice() {
        assert_eq!(diagnose(&working()), None);
    }

    #[test]
    fn missing_devices_come_first() {
        let probe = AccessProbe {
            devices_present: false,
            has_card: false,
            open: None,
            ..working()
        };
        assert_eq!(diagnose(&probe), Some(AccessProblem::NotPassedIn));
    }

    #[test]
    fn devices_without_a_card_mean_the_host_has_none() {
        let probe = AccessProbe {
            has_card: false,
            open: None,
            ..working()
        };
        assert_eq!(diagnose(&probe), Some(AccessProblem::NoSoundCard));
    }

    #[test]
    fn eperm_means_the_device_was_mounted_not_passed() {
        let probe = AccessProbe {
            open: Some(OpenOutcome::NotPermitted),
            ..working()
        };
        assert_eq!(diagnose(&probe), Some(AccessProblem::MountedAsVolume));
    }

    #[test]
    fn eacces_names_the_group_that_is_missing() {
        // Fedora's audio group, with the container given Debian's.
        let probe = AccessProbe {
            owner_groups: BTreeSet::from([63]),
            process_groups: BTreeSet::from([29, 999]),
            open: Some(OpenOutcome::PermissionDenied),
            ..working()
        };
        let problem = diagnose(&probe);
        assert_eq!(
            problem,
            Some(AccessProblem::WrongGroup { missing: vec![63] })
        );
        let advice = problem.map(|p| p.advice()).unwrap_or_default();
        assert!(advice.contains("Set AUDIO_GID=63 in .env"), "{advice}");
        assert!(advice.contains("--group-add 63"), "{advice}");
    }

    #[test]
    fn two_owning_groups_are_both_named() {
        let problem = AccessProblem::WrongGroup {
            missing: vec![29, 63],
        };
        let advice = problem.advice();
        assert!(advice.contains("List 29, 63 under group_add:"), "{advice}");
        assert!(advice.contains("--group-add 29 --group-add 63"), "{advice}");
    }

    #[test]
    fn eacces_with_the_right_group_blames_a_security_module() {
        let probe = AccessProbe {
            open: Some(OpenOutcome::PermissionDenied),
            ..working()
        };
        assert_eq!(diagnose(&probe), Some(AccessProblem::BlockedDespiteGroup));
        // Root skips file permissions, so a refusal cannot be about groups either.
        let root = AccessProbe {
            is_root: true,
            owner_groups: BTreeSet::from([63]),
            process_groups: BTreeSet::from([0]),
            open: Some(OpenOutcome::PermissionDenied),
            ..working()
        };
        assert_eq!(diagnose(&root), Some(AccessProblem::BlockedDespiteGroup));
    }

    #[test]
    fn other_failures_are_not_blamed_on_access() {
        let probe = AccessProbe {
            open: Some(OpenOutcome::Other),
            ..working()
        };
        assert_eq!(diagnose(&probe), None);
    }

    #[test]
    fn the_kernels_view_of_the_process_is_read_from_its_status() {
        let status = "Name:\ton-air-record\nUid:\t10001\t10001\t10001\t10001\nGid:\t999\t999\t999\t999\nGroups:\t29 63 \n";
        let (uid, groups) = parse_process_status(status);
        assert_eq!(uid, Some(10001));
        assert_eq!(groups, BTreeSet::from([29, 63, 999]));

        // An empty Groups: line is normal, and the effective group still counts.
        let (_, groups) = parse_process_status("Uid:\t0\t0\t0\t0\nGid:\t0\t5\t0\t0\nGroups:\n");
        assert_eq!(groups, BTreeSet::from([5]));
        assert_eq!(parse_process_status(""), (None, BTreeSet::new()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn probing_a_folder_finds_what_is_there() {
        let root = std::env::temp_dir().join(format!("oar-device-access-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        assert!(!probe(&root).devices_present);

        std::fs::create_dir_all(&root).expect("folder");
        std::fs::write(root.join("timer"), b"").expect("file");
        let empty = probe(&root);
        assert!(empty.devices_present);
        assert!(!empty.has_card);
        assert_eq!(diagnose(&empty), Some(AccessProblem::NoSoundCard));

        // A plain file stands in for the control device; opening it works, so there is nothing to say.
        std::fs::write(root.join("controlC0"), b"").expect("file");
        let card = probe(&root);
        assert!(card.has_card);
        assert_eq!(card.open, Some(OpenOutcome::Opened));
        assert_eq!(diagnose(&card), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}
