//! Contract tests: the golden fixtures in `contracts/` that pin the wire format between this service and
//! the browser.
//!
//! Each side used to test the wire against its own reading of the docs. `ws/protocol.rs` and
//! `frameCodec.ts` each checked the frame layout alone, and the DTOs here and the types in
//! `frontend/src/api/types.ts` were kept in step by hand, so a field renamed on one side left both suites
//! green. Now this module serialises fixed examples of everything the server sends (every stream control
//! message, every REST body the page reads, the error envelope, the wire enums, and audio frames through
//! the real encoder) exactly as the server does, and compares them with the checked in files. The
//! frontend's `src/api/__tests__/contract*.test.ts` reads the same files and proves its types have exactly
//! those keys and value kinds. A rename therefore fails here first, until the fixtures are regenerated with
//! `UPDATE_CONTRACTS=1 cargo test contract`, and then fails there, until the TypeScript follows.
//!
//! Values are fixed rather than taken from the clock, and every `Option` is shown both set and `null`
//! somewhere, because an `Option` serialises as `null` and a TypeScript type that forgot `| null` is only
//! caught by a fixture holding one. The frontend checks that coverage too.
//!
//! The other direction, what the browser sends, is written down by the frontend: `client-messages.json`
//! and `requests.json` hold what its code produces, which its own tests check. This module parses each one
//! into the real request type through the same deserialiser the route uses, and asserts every value
//! arrived. That last part matters: serde ignores a key it does not know, so an optional field sent under
//! the wrong name would parse cleanly and simply never be set.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use axum::extract::Query;
use axum::http::Uri;
use axum::response::IntoResponse;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::audio::activity::Sound;
use crate::audio::{build_encoder, FrameFormat};
use crate::dto::session_dto::StorageContext;
use crate::dto::timeline_dto::DirectionParam;
use crate::dto::update_dto::{InstallDto, ReleaseDto};
use crate::dto::{
    AuthStateResponse, BookmarkDto, BookmarkListResponse, CaptureDto, ChangePasswordRequest,
    CodeRequest, ConfirmPasswordRequest, CoverageDto, CreateBookmarkRequest, CreateUserRequest,
    CredentialsRequest, DeviceDto, DeviceListResponse, ExportPlanResponse, ExportQuery,
    HealthResponse, LevelsDto, NextSoundQuery, NextSoundResponse, PeaksQuery, PeaksResponse,
    RecordingDayDto, RecordingDaysResponse, RecoveryCodesResponse, SelectDeviceRequest, SessionDto,
    SessionListResponse, SetPasswordRequest, SettingsDto, SettingsPatchRequest, SoundDto,
    SoundsQuery, SoundsResponse, StatusResponse, StorageResponse, TestDirectoryRequest,
    TestDirectoryResponse, TimelineRangeResponse, TwoFactorSetupResponse, TwoFactorStatusResponse,
    UpdateBookmarkRequest, UpdateStatusResponse, UpdateUserRequest, UserDto, UserListResponse,
};
use crate::error::AppError;
use crate::models::update::{Channel, InstallKind, Release};
use crate::models::{
    AudioFrame, AuthMode, Bookmark, CaptureSnapshot, CaptureState, InputDevice, LevelSnapshot,
    ListenerAccount, ListenerActivity, ListenerEntry, PlayerState, RecordingSession, Role,
    Settings, SoundSensitivity, TimeRange, User,
};
use crate::repositories::{DaySummary, SegmentStorageStats, SessionSummary};
use crate::services::settings_service::DirectoryProbe;
use crate::services::timeline_service::{PeaksView, SeekDirection};
use crate::services::update_service::UpdateStatus;
use crate::services::{ExportPlan, RecordingDay};
use crate::ws::messages::{ClientMessage, ListenerView, ServerMessage, StreamMode};
use crate::ws::protocol::{decode_header, encode_audio_frame};

/// Set to rewrite the generated fixtures instead of comparing against them.
const UPDATE_VAR: &str = "UPDATE_CONTRACTS";

/// A moment in September 2025, the base of every timestamp here, so the files read as real times.
const T0: i64 = 1_757_030_400_000;

fn contracts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("contracts")
}

/// Keys sorted at every level. serde_json already sorts unless some dependency switches on its
/// `preserve_order` feature, and the files must not change shape because a dependency did.
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .into_iter()
                .map(|(key, inner)| (key, canonical(inner)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        other => other,
    }
}

fn render(value: Value) -> String {
    let mut text = serde_json::to_string_pretty(&canonical(value)).expect("fixtures serialise");
    text.push('\n');
    text
}

/// Serialise the way the server does: axum's `Json` is `serde_json` over the same `Serialize` impl.
fn wire(value: impl Serialize) -> Value {
    serde_json::to_value(value).expect("every wire type serialises")
}

/// Read a checked in fixture. Line endings are normalised, because a Windows checkout with `autocrlf`
/// hands back CRLF for a file that is LF in the repository.
fn read_fixture(file: &str) -> String {
    let path = contracts_dir().join(file);
    std::fs::read_to_string(&path)
        .map(|text| text.replace("\r\n", "\n"))
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

fn read_json_fixture(file: &str) -> Value {
    serde_json::from_str(&read_fixture(file))
        .unwrap_or_else(|error| panic!("contracts/{file} is not valid JSON: {error}"))
}

/// Compare a generated fixture with the checked in one, or rewrite it when asked to.
fn guard(file: &str, generated: &str) {
    let path = contracts_dir().join(file);
    if std::env::var_os(UPDATE_VAR).is_some() {
        std::fs::write(&path, generated)
            .unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
        return;
    }

    let checked_in = std::fs::read_to_string(&path)
        .map(|text| text.replace("\r\n", "\n"))
        .unwrap_or_default();
    if checked_in == generated {
        return;
    }

    let old: Vec<&str> = checked_in.lines().collect();
    let new: Vec<&str> = generated.lines().collect();
    let line = old
        .iter()
        .zip(new.iter())
        .position(|(was, now)| was != now)
        .unwrap_or(old.len().min(new.len()));
    panic!(
        "contracts/{file} no longer matches what the server sends.\n\
         First difference at line {}:\n  checked in: {}\n  generated:  {}\n\n\
         If the change is intended, rerun with `UPDATE_CONTRACTS=1 cargo test contract`, review the diff \
         of contracts/, then run `npm test` in frontend/, which fails until src/api/types.ts matches.",
        line + 1,
        old.get(line).unwrap_or(&"<end of file>"),
        new.get(line).unwrap_or(&"<end of file>"),
    );
}

/// Examples grouped under a name, the layout every generated JSON fixture shares.
type Examples = BTreeMap<String, Vec<Value>>;

fn render_examples(examples: Examples) -> String {
    render(wire(examples))
}

// ------------------------------------------------------------------------------------------------------
// Server messages
// ------------------------------------------------------------------------------------------------------

/// How many variants `ServerMessage` has. Kept beside the exhaustive match below, which stops compiling
/// when a variant is added, so the author is sent here to count it and give it an example.
const SERVER_MESSAGE_VARIANTS: usize = 11;

fn server_variant(message: &ServerMessage) -> usize {
    match message {
        ServerMessage::StreamInfo { .. } => 0,
        ServerMessage::Mode { .. } => 1,
        ServerMessage::SwitchedToLive { .. } => 2,
        ServerMessage::Gap { .. } => 3,
        ServerMessage::EndOfRecording { .. } => 4,
        ServerMessage::Level { .. } => 5,
        ServerMessage::Speed { .. } => 6,
        ServerMessage::Pong { .. } => 7,
        ServerMessage::Error { .. } => 8,
        ServerMessage::Listeners { .. } => 9,
        ServerMessage::ListenersHidden => 10,
    }
}

/// A signed in admin on the live feed, a guest in history over IPv6, and a listener who paused.
fn listener_entries() -> Vec<ListenerEntry> {
    vec![
        ListenerEntry {
            id: 1,
            account: Some(ListenerAccount {
                email: "owner@example.com".to_string(),
                role: Role::Admin,
            }),
            address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
            user_agent: Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)".to_string()),
            connected_at_ms: T0 + 3_000_000,
            activity: ListenerActivity::Live,
            player: PlayerState::Playing,
        },
        ListenerEntry {
            id: 2,
            account: None,
            address: IpAddr::V6(Ipv6Addr::LOCALHOST),
            user_agent: None,
            connected_at_ms: T0 + 3_100_000,
            activity: ListenerActivity::Playback { from_ms: T0 },
            player: PlayerState::Paused,
        },
        ListenerEntry {
            id: 3,
            account: Some(ListenerAccount {
                email: "listener@example.com".to_string(),
                role: Role::Listener,
            }),
            address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)),
            user_agent: Some("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)".to_string()),
            connected_at_ms: T0 + 3_200_000,
            activity: ListenerActivity::Paused,
            player: PlayerState::Idle,
        },
    ]
}

fn server_messages() -> Vec<ServerMessage> {
    vec![
        ServerMessage::StreamInfo {
            sample_rate: 48_000,
            channels: 1,
            frame_ms: 100,
            mode: StreamMode::Live,
            server_time_ms: T0 + 3_600_250,
            live_edge_ms: Some(T0 + 3_600_000),
            earliest_ms: Some(T0 - 86_400_000),
            capturing: true,
        },
        ServerMessage::StreamInfo {
            sample_rate: 44_100,
            channels: 2,
            frame_ms: 100,
            mode: StreamMode::Playback,
            server_time_ms: T0 + 3_600_250,
            live_edge_ms: None,
            earliest_ms: None,
            capturing: false,
        },
        ServerMessage::Mode {
            mode: StreamMode::Playback,
            position_ms: T0 + 60_000,
        },
        ServerMessage::Mode {
            mode: StreamMode::Paused,
            position_ms: T0 + 61_500,
        },
        ServerMessage::SwitchedToLive {
            timestamp_ms: T0 + 3_600_000,
        },
        ServerMessage::Gap {
            from_ms: T0 + 120_000,
            to_ms: T0 + 900_000,
        },
        ServerMessage::EndOfRecording {
            timestamp_ms: T0 + 1_800_000,
        },
        ServerMessage::Level {
            rms: 0.125,
            peak: 0.5,
        },
        ServerMessage::Speed { value: 1.5 },
        ServerMessage::Pong {
            client_time_ms: T0 + 3_600_000,
            server_time_ms: T0 + 3_600_012,
        },
        ServerMessage::Error {
            code: "bad_request".to_string(),
            message: "could not parse the control message: unknown variant `explode`".to_string(),
        },
        ServerMessage::Listeners {
            listeners: listener_entries()
                .into_iter()
                .map(ListenerView::from)
                .collect(),
        },
        ServerMessage::ListenersHidden,
    ]
}

#[test]
fn contract_server_messages() {
    let mut covered = BTreeSet::new();
    let mut examples = Examples::new();
    for message in server_messages() {
        covered.insert(server_variant(&message));
        let value = wire(&message);
        let tag = value["type"]
            .as_str()
            .expect("every server message carries a type tag")
            .to_string();
        examples.entry(tag).or_default().push(value);
    }
    assert_eq!(
        covered.len(),
        SERVER_MESSAGE_VARIANTS,
        "every ServerMessage variant needs an example in server_messages()"
    );
    guard("server-messages.json", &render_examples(examples));
}

// ------------------------------------------------------------------------------------------------------
// REST responses
// ------------------------------------------------------------------------------------------------------

fn capture(state: CaptureState, running: bool) -> CaptureDto {
    if running {
        CaptureSnapshot {
            state,
            session_id: Some(42),
            device_id: Some("Scarlett Solo USB".to_string()),
            device_name: Some("Scarlett Solo USB".to_string()),
            sample_rate: 48_000,
            channels: 1,
            frame_ms: 100,
            started_at_ms: Some(T0),
            dropped_frames: 3,
            error: None,
        }
        .into()
    } else {
        CaptureSnapshot {
            state,
            session_id: None,
            device_id: None,
            device_name: None,
            sample_rate: 0,
            channels: 0,
            frame_ms: 100,
            started_at_ms: None,
            dropped_frames: 0,
            error: Some("audio device error: the device is no longer available".to_string()),
        }
        .into()
    }
}

fn levels(rms: f32, peak: f32) -> LevelsDto {
    LevelSnapshot { rms, peak }.into()
}

fn user(id: i64, email: &str, role: Role, two_factor: bool) -> UserDto {
    User {
        id,
        email: email.to_string(),
        role,
        created_at_ms: T0 - 604_800_000 + id,
        two_factor,
    }
    .into()
}

fn bookmark(id: i64, label: &str, note: Option<&str>) -> BookmarkDto {
    Bookmark {
        id,
        timestamp_ms: T0 + id * 60_000,
        label: label.to_string(),
        note: note.map(str::to_string),
        created_at_ms: T0 + 3_600_000 + id,
    }
    .into()
}

fn sound(start_ms: i64, peak: u8) -> SoundDto {
    Sound {
        start_ms,
        end_ms: start_ms + 1_400,
        peak,
        seek_ms: start_ms - 1_000,
    }
    .into()
}

fn session(id: i64, ended: bool) -> SessionDto {
    SessionSummary {
        session: RecordingSession {
            id,
            device_id: "Scarlett Solo USB".to_string(),
            device_name: "Scarlett Solo USB".to_string(),
            sample_rate: 48_000,
            channels: 1,
            started_at_ms: T0 + id * 3_600_000,
            ended_at_ms: ended.then_some(T0 + id * 3_600_000 + 1_800_000),
        },
        segment_count: 180,
        bytes: 172_800_000,
    }
    .into()
}

fn release(tag: &str, prerelease: bool, published_at_ms: Option<i64>) -> Release {
    Release {
        tag: tag.to_string(),
        prerelease,
        draft: false,
        published_at_ms,
        notes: "### Added\n\n- Jump between the moments something was heard.".to_string(),
        url: format!("https://github.com/shibbirweb/on-air-record/releases/tag/{tag}"),
    }
}

/// An update answer built through the real conversion, with the host's OS pinned so the file is the same
/// on every machine that generates it.
fn update_status(status: UpdateStatus) -> UpdateStatusResponse {
    let mut response = UpdateStatusResponse::from(status);
    response.install.os = "linux";
    response
}

/// The error envelope exactly as `AppError::into_response` writes it.
async fn error_envelope(error: AppError) -> Value {
    let response = error.into_response();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error body");
    serde_json::from_slice(&bytes).expect("the error envelope is JSON")
}

async fn rest_responses() -> Examples {
    let mut examples = Examples::new();
    let mut add = |name: &str, values: Vec<Value>| {
        examples.insert(name.to_string(), values);
    };

    add(
        "health",
        vec![wire(HealthResponse {
            status: "ok",
            version: "1.4.0",
            uptime_ms: 86_400_000,
        })],
    );

    add(
        "status",
        vec![
            wire(StatusResponse {
                capture: capture(CaptureState::Recording, true),
                levels: levels(0.125, 0.5),
                listeners: 2,
                server_time_ms: T0 + 3_600_250,
                live_edge_ms: Some(T0 + 3_600_000),
            }),
            wire(StatusResponse {
                capture: capture(CaptureState::Error, false),
                levels: levels(0.0, 0.0),
                listeners: 0,
                server_time_ms: T0 + 3_600_250,
                live_edge_ms: None,
            }),
        ],
    );

    add(
        "settings",
        vec![
            wire(SettingsDto {
                input_device_id: Some("Scarlett Solo USB".to_string()),
                gain: 1.5,
                segment_seconds: 10,
                retention_hours: Some(72),
                auto_start: true,
                auto_start_delay_seconds: 20,
                frame_ms: 100,
                recording_sample_rate: Some(16_000),
                recordings_dir: Some("/srv/recordings".to_string()),
                effective_recordings_dir: "/srv/recordings".to_string(),
                check_for_updates: true,
                sound_sensitivity: SoundSensitivity::High,
            }),
            wire(SettingsDto {
                input_device_id: None,
                gain: 1.0,
                segment_seconds: 10,
                retention_hours: None,
                auto_start: false,
                auto_start_delay_seconds: 0,
                frame_ms: 100,
                recording_sample_rate: None,
                recordings_dir: None,
                effective_recordings_dir: "/var/lib/on-air-record/recordings".to_string(),
                check_for_updates: false,
                sound_sensitivity: SoundSensitivity::Medium,
            }),
        ],
    );

    add(
        "directoryTest",
        vec![
            wire(TestDirectoryResponse::from(DirectoryProbe {
                ok: true,
                resolved_path: "/srv/recordings".to_string(),
                exists: false,
                will_create: true,
                readable: true,
                writable: true,
                message: "The folder does not exist yet and will be created.".to_string(),
            })),
            wire(TestDirectoryResponse::from(DirectoryProbe {
                ok: false,
                resolved_path: "/root/recordings".to_string(),
                exists: true,
                will_create: false,
                readable: false,
                writable: false,
                message: "The service cannot write to this folder.".to_string(),
            })),
        ],
    );

    let storage_context = || StorageContext {
        data_dir: "/var/lib/on-air-record".to_string(),
        recordings_dir: "/var/lib/on-air-record/recordings".to_string(),
        sample_rate: 48_000,
        channels: 1,
    };
    add(
        "storage",
        vec![
            wire(StorageResponse::new(
                SegmentStorageStats {
                    segment_count: 8_640,
                    bytes: 8_294_400_000,
                    oldest_ms: Some(T0 - 86_400_000),
                    newest_ms: Some(T0),
                },
                &Settings {
                    retention_hours: Some(72),
                    ..Settings::default()
                },
                storage_context(),
            )),
            wire(StorageResponse::new(
                SegmentStorageStats {
                    segment_count: 0,
                    bytes: 0,
                    oldest_ms: None,
                    newest_ms: None,
                },
                &Settings {
                    retention_hours: None,
                    ..Settings::default()
                },
                storage_context(),
            )),
        ],
    );

    add(
        "sessions",
        vec![
            wire(SessionListResponse {
                sessions: vec![session(1, true), session(2, false)],
            }),
            wire(SessionListResponse {
                sessions: Vec::new(),
            }),
        ],
    );

    add(
        "devices",
        vec![wire(DeviceListResponse {
            devices: vec![
                DeviceDto::from_model(
                    InputDevice {
                        id: "MacBook Pro Microphone".to_string(),
                        name: "MacBook Pro Microphone".to_string(),
                        is_default: true,
                        available: true,
                        channels: 1,
                        sample_rate: 48_000,
                    },
                    Some("Scarlett Solo USB"),
                ),
                DeviceDto::from_model(
                    InputDevice {
                        id: "Scarlett Solo USB".to_string(),
                        name: "Scarlett Solo USB".to_string(),
                        is_default: false,
                        available: false,
                        channels: 2,
                        sample_rate: 44_100,
                    },
                    Some("Scarlett Solo USB"),
                ),
            ],
        })],
    );

    add(
        "timelineRange",
        vec![
            wire(TimelineRangeResponse {
                earliest_ms: Some(T0 - 86_400_000),
                latest_ms: Some(T0 + 3_590_000),
                live_edge_ms: Some(T0 + 3_600_000),
                server_time_ms: T0 + 3_600_250,
                coverage: vec![
                    CoverageDto::from(TimeRange::new(T0 - 86_400_000, T0 - 43_200_000)),
                    CoverageDto::from(TimeRange::new(T0, T0 + 3_590_000)),
                ],
            }),
            wire(TimelineRangeResponse {
                earliest_ms: None,
                latest_ms: None,
                live_edge_ms: None,
                server_time_ms: T0 + 3_600_250,
                coverage: Vec::new(),
            }),
        ],
    );

    add(
        "recordingDays",
        vec![wire(RecordingDaysResponse {
            days: vec![RecordingDayDto::from(RecordingDay {
                summary: DaySummary {
                    day: "2025-09-05".to_string(),
                    start_ms: T0,
                    end_ms: T0 + 3_590_000,
                    segment_count: 359,
                    bytes: 344_640_000,
                    recorded_ms: 3_590_000,
                },
                day_start_ms: T0 - 7_200_000,
                day_end_ms: T0 + 79_200_000,
            })],
        })],
    );

    add(
        "peaks",
        vec![wire(PeaksResponse::from(PeaksView {
            from_ms: T0,
            to_ms: T0 + 1_000,
            bucket_ms: 100,
            values: vec![0, 3, 12, 255, 90, 4, 0, 0, 1, 2],
        }))],
    );

    add(
        "sounds",
        vec![wire(SoundsResponse {
            from_ms: T0,
            to_ms: T0 + 3_600_000,
            sensitivity: SoundSensitivity::Medium,
            sounds: vec![sound(T0 + 61_000, 140), sound(T0 + 905_000, 220)],
        })],
    );

    add(
        "nextSound",
        vec![
            wire(NextSoundResponse {
                sound: Some(sound(T0 + 905_000, 220)),
            }),
            wire(NextSoundResponse { sound: None }),
        ],
    );

    add(
        "bookmark",
        vec![
            wire(bookmark(7, "Doorbell", Some("Parcel for next door"))),
            wire(bookmark(8, "Dog barking", None)),
        ],
    );

    add(
        "bookmarks",
        vec![wire(BookmarkListResponse {
            bookmarks: vec![
                bookmark(7, "Doorbell", Some("Parcel for next door")),
                bookmark(8, "Dog barking", None),
            ],
        })],
    );

    add(
        "exportPlan",
        vec![wire(ExportPlanResponse::from(ExportPlan {
            range: TimeRange::new(T0, T0 + 60_000),
            sample_rate: 16_000,
            channels: 1,
            data_bytes: 1_920_000,
            total_bytes: 1_920_044,
            mixed_rates: true,
        }))],
    );

    // Signed in, the login outcome that still needs a code, and an open recorder.
    add(
        "authState",
        vec![
            wire(AuthStateResponse {
                mode: AuthMode::Accounts,
                user: Some(user(1, "owner@example.com", Role::Admin, true)),
                pending_two_factor: false,
            }),
            wire(AuthStateResponse {
                mode: AuthMode::Accounts,
                user: None,
                pending_two_factor: true,
            }),
            wire(AuthStateResponse {
                mode: AuthMode::Open,
                user: None,
                pending_two_factor: false,
            }),
        ],
    );

    add(
        "user",
        vec![wire(user(2, "listener@example.com", Role::Listener, false))],
    );

    add(
        "users",
        vec![wire(UserListResponse {
            users: vec![
                user(1, "owner@example.com", Role::Admin, true),
                user(2, "listener@example.com", Role::Listener, false),
            ],
        })],
    );

    add(
        "twoFactorStatus",
        vec![wire(TwoFactorStatusResponse {
            enabled: true,
            recovery_codes_left: 8,
        })],
    );

    add(
        "twoFactorSetup",
        vec![wire(TwoFactorSetupResponse {
            secret_key: "JBSW Y3DP EHPK 3PXP".to_string(),
            otpauth_uri: "otpauth://totp/On%20Air%20Record:owner%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=On%20Air%20Record".to_string(),
            qr_svg: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 29 29\"></svg>".to_string(),
        })],
    );

    add(
        "recoveryCodes",
        vec![wire(RecoveryCodesResponse {
            recovery_codes: vec![
                "4f7c-9a21".to_string(),
                "b83e-06d5".to_string(),
                "c1a9-7e44".to_string(),
            ],
        })],
    );

    add(
        "updateStatus",
        vec![
            wire(update_status(UpdateStatus {
                current: "1.4.0".to_string(),
                channel: Channel::Beta,
                automatic: true,
                checked_at_ms: Some(T0 + 3_600_000),
                error: None,
                newer: vec![
                    release("v1.5.0-beta.1", true, Some(T0 + 1_000)),
                    release("v1.4.1", false, None),
                ],
                install: InstallKind::Installer {
                    dir: "/home/me/on-air-record".to_string(),
                },
                target: "x86_64-unknown-linux-gnu",
            })),
            wire(update_status(UpdateStatus {
                current: "1.4.0".to_string(),
                channel: Channel::Stable,
                automatic: true,
                checked_at_ms: Some(T0 + 3_600_000),
                error: None,
                newer: vec![release("v1.4.1", false, None)],
                install: InstallKind::Systemd,
                target: "x86_64-unknown-linux-gnu",
            })),
            wire(update_status(UpdateStatus {
                current: "1.4.0".to_string(),
                channel: Channel::Stable,
                automatic: false,
                checked_at_ms: None,
                error: Some("could not reach GitHub: timed out".to_string()),
                newer: Vec::new(),
                install: InstallKind::Docker,
                target: "aarch64-unknown-linux-gnu",
            })),
        ],
    );

    add(
        "error",
        vec![
            error_envelope(AppError::bad_request("the role must be admin or listener")).await,
            error_envelope(AppError::unauthorized("sign in to continue")).await,
            error_envelope(AppError::too_many_requests(
                "too many attempts, wait a minute",
            ))
            .await,
        ],
    );

    examples
}

#[tokio::test]
async fn contract_rest_responses() {
    guard("responses.json", &render_examples(rest_responses().await));
}

// ------------------------------------------------------------------------------------------------------
// Wire enums
// ------------------------------------------------------------------------------------------------------

// Each list below is followed by an exhaustive match over the same enum. Adding a variant stops that
// match compiling, which sends the author here to add it to the list, and so to the fixture the
// frontend compares its string unions with.

fn every_capture_state() -> Vec<CaptureState> {
    let all = vec![
        CaptureState::Idle,
        CaptureState::Starting,
        CaptureState::Recording,
        CaptureState::Error,
    ];
    for state in &all {
        match state {
            CaptureState::Idle
            | CaptureState::Starting
            | CaptureState::Recording
            | CaptureState::Error => {}
        }
    }
    all
}

fn every_role() -> Vec<Role> {
    let all = vec![Role::Admin, Role::Listener];
    for role in &all {
        match role {
            Role::Admin | Role::Listener => {}
        }
    }
    all
}

fn every_auth_mode() -> Vec<AuthMode> {
    let all = vec![AuthMode::Undecided, AuthMode::Open, AuthMode::Accounts];
    for mode in &all {
        match mode {
            AuthMode::Undecided | AuthMode::Open | AuthMode::Accounts => {}
        }
    }
    all
}

fn every_stream_mode() -> Vec<StreamMode> {
    let all = vec![StreamMode::Live, StreamMode::Playback, StreamMode::Paused];
    for mode in &all {
        match mode {
            StreamMode::Live | StreamMode::Playback | StreamMode::Paused => {}
        }
    }
    all
}

fn every_player_state() -> Vec<PlayerState> {
    let all = vec![PlayerState::Idle, PlayerState::Playing, PlayerState::Paused];
    for state in &all {
        match state {
            PlayerState::Idle | PlayerState::Playing | PlayerState::Paused => {}
        }
    }
    all
}

fn every_sound_sensitivity() -> Vec<SoundSensitivity> {
    let all = vec![
        SoundSensitivity::Low,
        SoundSensitivity::Medium,
        SoundSensitivity::High,
    ];
    for sensitivity in &all {
        match sensitivity {
            SoundSensitivity::Low | SoundSensitivity::Medium | SoundSensitivity::High => {}
        }
    }
    all
}

fn every_channel() -> Vec<Channel> {
    let all = vec![Channel::Stable, Channel::Beta];
    for channel in &all {
        match channel {
            Channel::Stable | Channel::Beta => {}
        }
    }
    all
}

fn every_install_kind() -> Vec<InstallKind> {
    let all = vec![
        InstallKind::Installer {
            dir: "/home/me/on-air-record".to_string(),
        },
        InstallKind::Systemd,
        InstallKind::Docker,
        InstallKind::Manual,
    ];
    for kind in &all {
        match kind {
            InstallKind::Installer { .. }
            | InstallKind::Systemd
            | InstallKind::Docker
            | InstallKind::Manual => {}
        }
    }
    all
}

/// The word an install kind travels as, through the real conversion rather than a copy of its table.
fn install_kind_word(kind: InstallKind) -> Value {
    let response = UpdateStatusResponse::from(UpdateStatus {
        current: "1.4.0".to_string(),
        channel: Channel::Stable,
        automatic: true,
        checked_at_ms: None,
        error: None,
        newer: Vec::new(),
        install: kind,
        target: "x86_64-unknown-linux-gnu",
    });
    let InstallDto { kind, .. } = response.install;
    wire(kind)
}

fn words<T: Serialize>(values: Vec<T>) -> Vec<Value> {
    values.into_iter().map(wire).collect()
}

#[test]
fn contract_enums() {
    let mut examples = Examples::new();
    examples.insert("authMode".to_string(), words(every_auth_mode()));
    examples.insert("captureState".to_string(), words(every_capture_state()));
    examples.insert("channel".to_string(), words(every_channel()));
    examples.insert(
        "installKind".to_string(),
        every_install_kind()
            .into_iter()
            .map(install_kind_word)
            .collect(),
    );
    examples.insert("playerState".to_string(), words(every_player_state()));
    examples.insert("role".to_string(), words(every_role()));
    examples.insert(
        "soundSensitivity".to_string(),
        words(every_sound_sensitivity()),
    );
    examples.insert("streamMode".to_string(), words(every_stream_mode()));
    guard("enums.json", &render_examples(examples));
}

// ------------------------------------------------------------------------------------------------------
// Binary audio frames
// ------------------------------------------------------------------------------------------------------

struct FrameExample {
    name: &'static str,
    frame: AudioFrame,
}

fn frame_examples() -> Vec<FrameExample> {
    vec![
        FrameExample {
            name: "live48kMono",
            frame: AudioFrame::from_samples(
                T0,
                48_000,
                1,
                vec![0, 1, -1, 16_384, 32_767, -32_768],
                true,
            ),
        },
        FrameExample {
            name: "historic44kStereo",
            frame: AudioFrame::from_samples(
                T0 + 100,
                44_100,
                2,
                vec![100, -100, 2_000, -2_000, 32_767, -32_768],
                false,
            ),
        },
        FrameExample {
            name: "historicBeforeTheEpoch",
            frame: AudioFrame::from_samples(-86_400_000, 16_000, 1, vec![5, -5], false),
        },
        // The browser reads the timestamp through a `BigInt` into a `Number`, which is exact only up to
        // 2^53 - 1. Real timestamps are far below it; this pins the largest one that survives.
        FrameExample {
            name: "liveLargestSafeTimestamp",
            frame: AudioFrame::from_samples(9_007_199_254_740_991, 48_000, 1, vec![1], true),
        },
        FrameExample {
            name: "liveEmpty",
            frame: AudioFrame::from_samples(T0 + 200, 48_000, 1, Vec::new(), true),
        },
    ]
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn contract_audio_frames() {
    let encoder = build_encoder(FrameFormat::PcmS16);
    let mut frames = BTreeMap::new();
    for example in frame_examples() {
        let message = encode_audio_frame(&example.frame, encoder.as_ref());
        let header =
            decode_header(&message).expect("the encoder writes a header its decoder reads");
        let magic = u32::from_le_bytes([message[0], message[1], message[2], message[3]]);
        frames.insert(
            example.name,
            json!({
                "hex": hex(&message),
                "byteLength": message.len(),
                "header": {
                    "magic": magic,
                    "version": header.version,
                    "formatCode": header.format_code,
                    "channels": header.channels,
                    "flags": header.flags,
                    "live": header.is_live(),
                    "sampleRate": header.sample_rate,
                    "sampleCount": header.sample_count,
                    "timestampMs": header.timestamp_ms,
                },
                "samples": example.frame.samples.as_slice(),
            }),
        );
    }
    guard("audio-frames.json", &render(wire(frames)));
}

// ------------------------------------------------------------------------------------------------------
// What the browser sends: stream control messages
// ------------------------------------------------------------------------------------------------------

const CLIENT_MESSAGE_VARIANTS: usize = 7;

fn int(value: &Value, key: &str) -> i64 {
    value[key]
        .as_i64()
        .unwrap_or_else(|| panic!("`{key}` in {value} is not an integer"))
}

fn float(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .unwrap_or_else(|| panic!("`{key}` in {value} is not a number"))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("`{key}` in {value} is not a string"))
}

fn optional_text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match &value[key] {
        Value::Null => None,
        Value::String(inner) => Some(inner.as_str()),
        other => panic!("`{key}` is {other}, not a string or null"),
    }
}

fn parse_json<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

/// The keys of `value` the server actually reads.
///
/// serde skips a key it does not know, so a field sent under a name the server has never heard of parses
/// cleanly and is silently lost; checking the values that did arrive cannot see it, because the missing
/// field is `None` on both sides of the comparison. Instead each key is taken out in turn and the rest
/// parsed again: a key whose absence changes the result, or makes it fail, is one the server reads. The
/// comparison is on the `Debug` output, which every request type derives.
fn read_keys<T: Debug>(
    value: &Value,
    full: &T,
    parse_one: impl Fn(&Value) -> Result<T, String>,
) -> BTreeSet<String> {
    let whole = format!("{full:?}");
    let Some(map) = value.as_object() else {
        return BTreeSet::new();
    };
    map.keys()
        .filter(|key| {
            let mut without = map.clone();
            without.remove(key.as_str());
            match parse_one(&Value::Object(without)) {
                Ok(parsed) => format!("{parsed:?}") != whole,
                Err(_) => true,
            }
        })
        .cloned()
        .collect()
}

/// Parse what the frontend sends into a server type, noting which of its keys the server read.
fn parse<T: DeserializeOwned + Debug>(what: &str, value: &Value, read: &mut BTreeSet<String>) -> T {
    let full: T = parse_json(value).unwrap_or_else(|error| {
        panic!("the server cannot parse {what} as the frontend sends it: {error}\n{value}")
    });
    read.extend(read_keys(value, &full, parse_json::<T>));
    full
}

fn keys_of(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Every key sent under `name`, across all its examples, must be one the server read in at least one of
/// them. A key only matters in some examples (a `null` note means the same as none on create), so the
/// sets are pooled rather than compared example by example.
fn assert_all_read(what: &str, sent: &BTreeSet<String>, read: &BTreeSet<String>) {
    let ignored: Vec<&String> = sent.difference(read).collect();
    assert!(
        ignored.is_empty(),
        "the frontend sends {ignored:?} in {what}, which the server never reads: a renamed field?"
    );
}

fn player_word(state: PlayerState) -> Value {
    wire(state)
}

#[test]
fn contract_client_messages_parse_into_the_server_types() {
    let fixture = read_json_fixture("client-messages.json");
    let groups = fixture
        .as_object()
        .expect("client-messages.json holds an object of examples by type");

    let mut covered = BTreeSet::new();
    for (name, examples) in groups {
        let examples = examples.as_array().expect("each type holds a list");
        assert!(!examples.is_empty(), "`{name}` has no example");
        let mut sent = BTreeSet::new();
        let mut read = BTreeSet::new();
        for example in examples {
            assert_eq!(
                text(example, "type"),
                name,
                "an example filed under the wrong type"
            );
            sent.extend(keys_of(example));
            let message: ClientMessage =
                parse(&format!("the `{name}` message"), example, &mut read);
            let variant = match message {
                ClientMessage::Live => 0,
                ClientMessage::Seek { timestamp_ms } => {
                    assert_eq!(timestamp_ms, int(example, "timestampMs"));
                    1
                }
                ClientMessage::Pause => 2,
                ClientMessage::Resume => 3,
                ClientMessage::Speed { value } => {
                    assert_eq!(f64::from(value), float(example, "value"));
                    4
                }
                ClientMessage::Ping { client_time_ms } => {
                    assert_eq!(client_time_ms, int(example, "clientTimeMs"));
                    5
                }
                ClientMessage::Player { state } => {
                    assert_eq!(player_word(state), example["state"]);
                    6
                }
            };
            covered.insert(variant);
        }
        assert_all_read(&format!("the `{name}` message"), &sent, &read);
    }
    assert_eq!(
        covered.len(),
        CLIENT_MESSAGE_VARIANTS,
        "client-messages.json must hold an example of every ClientMessage variant"
    );
}

// ------------------------------------------------------------------------------------------------------
// What the browser sends: REST request bodies and query strings
// ------------------------------------------------------------------------------------------------------

/// Every `api` method in `frontend/src/api/client.ts` that sends a JSON body, by the name it has there.
const BODY_CALLS: [&str; 15] = [
    "changePassword",
    "createBookmark",
    "createUser",
    "disableTwoFactor",
    "enableTwoFactor",
    "logIn",
    "regenerateRecoveryCodes",
    "selectDevice",
    "setUp",
    "setUserPassword",
    "testRecordingsDir",
    "updateBookmark",
    "updateSettings",
    "updateUserRole",
    "verifyLogin",
];

/// Every `api` method that sends its arguments in a query string.
const QUERY_CALLS: [&str; 5] = ["exportPlan", "exportUrl", "nextSound", "peaks", "sounds"];

/// A field the frontend sends as a value or as `null`, which the server reads into a nested option so
/// that `null` means "clear it" and absence means "leave it alone".
fn nested_text(body: &Value, key: &str) -> Option<Option<String>> {
    body.get(key)
        .map(|inner| inner.as_str().map(str::to_string))
}

fn nested_u32(body: &Value, key: &str) -> Option<Option<u32>> {
    body.get(key).map(|inner| {
        inner
            .as_u64()
            .map(|number| u32::try_from(number).expect("fits in u32"))
    })
}

fn check_body(call: &str, body: &Value, read: &mut BTreeSet<String>) {
    let what = format!("the body of `api.{call}`");
    match call {
        "logIn" | "setUp" => {
            let request: CredentialsRequest = parse(&what, body, read);
            assert_eq!(request.email, text(body, "email"));
            assert_eq!(request.password, text(body, "password"));
        }
        "verifyLogin" | "enableTwoFactor" => {
            let request: CodeRequest = parse(&what, body, read);
            assert_eq!(request.code, text(body, "code"));
        }
        "disableTwoFactor" | "regenerateRecoveryCodes" => {
            let request: ConfirmPasswordRequest = parse(&what, body, read);
            assert_eq!(request.password, text(body, "password"));
        }
        "changePassword" => {
            let request: ChangePasswordRequest = parse(&what, body, read);
            assert_eq!(request.current_password, text(body, "currentPassword"));
            assert_eq!(request.new_password, text(body, "newPassword"));
        }
        "createUser" => {
            let request: CreateUserRequest = parse(&what, body, read);
            assert_eq!(request.email, text(body, "email"));
            assert_eq!(request.password, text(body, "password"));
            let role = request
                .parsed_role()
                .expect("the role is one the server knows");
            assert_eq!(wire(role), body["role"]);
        }
        "updateUserRole" => {
            let request: UpdateUserRequest = parse(&what, body, read);
            let role = request
                .parsed_role()
                .expect("the role is one the server knows");
            assert_eq!(wire(role), body["role"]);
        }
        "setUserPassword" => {
            let request: SetPasswordRequest = parse(&what, body, read);
            assert_eq!(request.password, text(body, "password"));
        }
        "selectDevice" => {
            let request: SelectDeviceRequest = parse(&what, body, read);
            assert!(body.get("deviceId").is_some(), "deviceId is always sent");
            assert_eq!(
                request.device_id.as_deref(),
                optional_text(body, "deviceId")
            );
        }
        "testRecordingsDir" => {
            let request: TestDirectoryRequest = parse(&what, body, read);
            assert!(body.get("path").is_some(), "path is always sent");
            assert_eq!(request.path.as_deref(), optional_text(body, "path"));
        }
        "createBookmark" => {
            let request: CreateBookmarkRequest = parse(&what, body, read);
            assert_eq!(request.timestamp_ms, int(body, "timestampMs"));
            assert_eq!(request.label, text(body, "label"));
            assert_eq!(request.note.as_deref(), optional_text(body, "note"));
        }
        "updateBookmark" => {
            let request: UpdateBookmarkRequest = parse(&what, body, read);
            assert_eq!(
                request.timestamp_ms,
                body.get("timestampMs").and_then(Value::as_i64)
            );
            assert_eq!(
                request.label,
                body.get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            );
            assert_eq!(request.note, nested_text(body, "note"));
        }
        "updateSettings" => {
            let request: SettingsPatchRequest = parse(&what, body, read);
            assert_eq!(request.input_device_id, nested_text(body, "inputDeviceId"));
            assert_eq!(
                request.gain.map(f64::from),
                body.get("gain").and_then(Value::as_f64)
            );
            assert_eq!(
                request.segment_seconds.map(u64::from),
                body.get("segmentSeconds").and_then(Value::as_u64)
            );
            assert_eq!(request.retention_hours, nested_u32(body, "retentionHours"));
            assert_eq!(
                request.auto_start,
                body.get("autoStart").and_then(Value::as_bool)
            );
            assert_eq!(
                request.auto_start_delay_seconds.map(u64::from),
                body.get("autoStartDelaySeconds").and_then(Value::as_u64)
            );
            assert_eq!(
                request.frame_ms.map(u64::from),
                body.get("frameMs").and_then(Value::as_u64)
            );
            assert_eq!(
                request.recording_sample_rate,
                nested_u32(body, "recordingSampleRate")
            );
            assert_eq!(request.recordings_dir, nested_text(body, "recordingsDir"));
            assert_eq!(
                request.check_for_updates,
                body.get("checkForUpdates").and_then(Value::as_bool)
            );
            assert_eq!(
                request.sound_sensitivity.map(wire),
                body.get("soundSensitivity").cloned()
            );
        }
        other => panic!(
            "requests.json has a body for `api.{other}`, which this test does not know: add it to \
             BODY_CALLS and parse it here into the request type its route takes"
        ),
    }
}

/// Parse a query the way the route does, through axum's own `Query` extractor on a real URI.
fn parse_query<T: DeserializeOwned>(path: &str, fields: &Value) -> Result<T, String> {
    let pairs: Vec<String> = fields
        .as_object()
        .expect("a query is an object of fields")
        .iter()
        .map(|(key, value)| match value {
            Value::String(inner) => format!("{key}={inner}"),
            other => format!("{key}={other}"),
        })
        .collect();
    let uri: Uri = format!("/api{path}?{}", pairs.join("&"))
        .parse()
        .map_err(|error| format!("{error}"))?;
    Query::<T>::try_from_uri(&uri)
        .map(|query| query.0)
        .map_err(|error| format!("{uri}: {error}"))
}

fn query<T: DeserializeOwned + Debug>(
    call: &str,
    path: &str,
    fields: &Value,
    read: &mut BTreeSet<String>,
) -> T {
    let full: T = parse_query(path, fields).unwrap_or_else(|error| {
        panic!("the server cannot parse the query of `api.{call}`: {error}")
    });
    read.extend(read_keys(fields, &full, |without| {
        parse_query::<T>(path, without)
    }));
    full
}

fn check_query(call: &str, path: &str, fields: &Value, read: &mut BTreeSet<String>) {
    match call {
        "peaks" => {
            assert_eq!(path, "/timeline/peaks");
            let parsed: PeaksQuery = query(call, path, fields, read);
            assert_eq!(parsed.from_ms, int(fields, "fromMs"));
            assert_eq!(parsed.to_ms, int(fields, "toMs"));
            assert_eq!(parsed.buckets as i64, int(fields, "buckets"));
        }
        "sounds" => {
            assert_eq!(path, "/timeline/sounds");
            let parsed: SoundsQuery = query(call, path, fields, read);
            assert_eq!(parsed.from_ms, int(fields, "fromMs"));
            assert_eq!(parsed.to_ms, int(fields, "toMs"));
        }
        "nextSound" => {
            assert_eq!(path, "/timeline/sounds/next");
            let parsed: NextSoundQuery = query(call, path, fields, read);
            assert_eq!(parsed.from_ms, int(fields, "fromMs"));
            let expected = match text(fields, "direction") {
                "forward" => SeekDirection::Forward,
                "backward" => SeekDirection::Backward,
                other => panic!("the frontend sends direction `{other}`"),
            };
            let direction: DirectionParam = parsed.direction;
            assert_eq!(SeekDirection::from(direction), expected);
        }
        "exportPlan" | "exportUrl" => {
            let parsed: ExportQuery = query(call, path, fields, read);
            assert_eq!(parsed.from_ms, int(fields, "fromMs"));
            assert_eq!(parsed.to_ms, int(fields, "toMs"));
        }
        other => panic!(
            "requests.json has a query for `api.{other}`, which this test does not know: add it to \
             QUERY_CALLS and parse it here into the query type its route takes"
        ),
    }
}

fn names(group: &Value) -> BTreeSet<String> {
    group
        .as_object()
        .expect("a group of calls")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn contract_request_bodies_and_queries_parse_into_the_server_types() {
    let fixture = read_json_fixture("requests.json");

    let expected_bodies: BTreeSet<String> =
        BODY_CALLS.iter().map(|name| name.to_string()).collect();
    assert_eq!(
        names(&fixture["bodies"]),
        expected_bodies,
        "the calls with a body in requests.json and the ones this test parses differ"
    );
    let expected_queries: BTreeSet<String> =
        QUERY_CALLS.iter().map(|name| name.to_string()).collect();
    assert_eq!(
        names(&fixture["queries"]),
        expected_queries,
        "the calls with a query in requests.json and the ones this test parses differ"
    );

    for (call, examples) in fixture["bodies"].as_object().expect("bodies") {
        let examples = examples.as_array().expect("each call holds a list");
        assert!(!examples.is_empty(), "`{call}` has no example");
        let mut sent = BTreeSet::new();
        let mut read = BTreeSet::new();
        for example in examples {
            sent.extend(keys_of(&example["body"]));
            check_body(call, &example["body"], &mut read);
        }
        assert_all_read(&format!("the body of `api.{call}`"), &sent, &read);
    }
    for (call, examples) in fixture["queries"].as_object().expect("queries") {
        let examples = examples.as_array().expect("each call holds a list");
        assert!(!examples.is_empty(), "`{call}` has no example");
        let mut sent = BTreeSet::new();
        let mut read = BTreeSet::new();
        for example in examples {
            sent.extend(keys_of(&example["query"]));
            check_query(call, text(example, "path"), &example["query"], &mut read);
        }
        assert_all_read(&format!("the query of `api.{call}`"), &sent, &read);
    }
}

// ------------------------------------------------------------------------------------------------------
// The guard itself
// ------------------------------------------------------------------------------------------------------

#[test]
fn contract_fixtures_are_rendered_with_sorted_keys_and_a_trailing_newline() {
    let rendered = render(json!({ "b": 1, "a": { "d": [ { "f": 1, "e": 2 } ], "c": null } }));
    assert_eq!(
        rendered,
        "{\n  \"a\": {\n    \"c\": null,\n    \"d\": [\n      {\n        \"e\": 2,\n        \"f\": 1\n      }\n    ]\n  },\n  \"b\": 1\n}\n"
    );
}

#[test]
fn contract_release_dto_is_what_the_update_answer_nests() {
    // `available` is built field by field in the conversion rather than through `From<Release>`, so a
    // field added to one and not the other would make the two shapes differ. The fixture compares them
    // as the same TypeScript type, so they must serialise alike.
    let from_release = wire(ReleaseDto::from(release("v1.5.0", false, Some(T0))));
    let response = UpdateStatusResponse::from(UpdateStatus {
        current: "1.4.0".to_string(),
        channel: Channel::Stable,
        automatic: true,
        checked_at_ms: None,
        error: None,
        newer: vec![release("v1.5.0", false, Some(T0))],
        install: InstallKind::Manual,
        target: "x86_64-unknown-linux-gnu",
    });
    assert_eq!(wire(response.available), from_release);
}
