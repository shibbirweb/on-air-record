//! The sound finder end to end, from audio samples to the timeline, with everything real but the microphone.
//!
//! The detector's own tests feed it envelopes written by hand, which prove its rules but not that real
//! audio produces the envelopes those rules expect. Here a generated scene (room noise, mains hum, speech
//! like syllables, a click and a door) goes through the recorder exactly as captured frames do: envelope
//! computed while recording, segments written to disk and indexed in SQLite. Then the timeline service has
//! to find the right sounds in what was stored. Only the microphone driver is left out, which CI has no
//! way to provide.

use std::f64::consts::TAU;
use std::sync::Arc;

use crossbeam_channel::bounded;

use crate::audio::{build_encoder, FrameFormat, SegmentLayout};
use crate::config::AppConfig;
use crate::db::Database;
use crate::models::{AudioFrame, SessionDraft, SoundSensitivity};
use crate::repositories::{SegmentRepository, SessionRepository, SettingsRepository};
use crate::services::recorder_service::{RecorderContext, RecorderService};
use crate::services::{BroadcastHub, SettingsService, TimelineService};

const RATE: u32 = 48_000;
const FRAME_MS: i64 = 100;
const FRAME_SAMPLES: usize = (RATE as usize * FRAME_MS as usize) / 1000;
const SCENE_SECONDS: i64 = 180;
/// A fixed start, so a failure reproduces exactly.
const T0: i64 = 1_757_000_000_000;

const SPEECH_FROM_S: f64 = 60.0;
const SPEECH_TO_S: f64 = 66.0;
const CLICK_AT_S: f64 = 100.0;
const DOOR_AT_S: f64 = 150.0;

/// Deterministic noise, uniform in -1..1, so the scene is identical on every run and every platform.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 as f64 / u64::MAX as f64) * 2.0 - 1.0
    }
}

/// The scene at one instant, as a sample in -1..1, before `gain`.
fn scene(seconds: f64, noise: &mut Noise) -> f64 {
    // What every recording has: the room's own hiss, about -53 dBFS, and mains hum just under it.
    let mut sample = 0.004 * noise.next() + 0.003 * (TAU * 50.0 * seconds).sin();

    // Speech: 250 ms syllables with 150 ms between them, noise shaped by a voice-like pitch, about -27 dBFS.
    if (SPEECH_FROM_S..SPEECH_TO_S).contains(&seconds) && (seconds * 1000.0) % 400.0 < 250.0 {
        let voicing = 0.5 + 0.5 * (TAU * 140.0 * seconds).sin();
        sample += 0.12 * noise.next() * voicing;
    }
    // A click: 5 ms at nearly full scale, loud but far too short to be anything anybody wants to find.
    if (CLICK_AT_S..CLICK_AT_S + 0.005).contains(&seconds) {
        sample += 0.9;
    }
    // A door: a 60 Hz thud dying away over a few hundred milliseconds.
    if seconds >= DOOR_AT_S {
        let since = seconds - DOOR_AT_S;
        sample += 0.6 * (-since / 0.12).exp() * (TAU * 60.0 * since).sin();
    }
    sample
}

struct Recorded {
    timeline: TimelineService,
    data_dir: std::path::PathBuf,
}

impl Drop for Recorded {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

/// Record the scene at `gain` through the real recorder into a fresh data directory.
fn record_scene(name: &str, gain: f64) -> Recorded {
    let data_dir =
        std::env::temp_dir().join(format!("oar-sound-pipeline-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir).expect("data dir");

    let config = Arc::new(AppConfig {
        data_dir: data_dir.clone(),
        ..AppConfig::default()
    });
    let database = Arc::new(Database::open(&config.database_path()).expect("database"));
    let settings = Arc::new(
        SettingsService::load(
            Arc::new(SettingsRepository::new(database.clone())),
            config.clone(),
        )
        .expect("settings"),
    );
    let segments = Arc::new(SegmentRepository::new(database.clone()));
    let session = SessionRepository::new(database)
        .create(&SessionDraft {
            device_id: "generated".to_string(),
            device_name: "generated scene".to_string(),
            sample_rate: RATE,
            channels: 1,
            started_at_ms: T0,
        })
        .expect("session");
    let hub = Arc::new(BroadcastHub::new());

    let (sender, receiver) = bounded(64);
    let recorder = RecorderService::spawn(
        RecorderContext {
            config: config.clone(),
            segments: segments.clone(),
            settings,
            hub: hub.clone(),
            encoder: build_encoder(FrameFormat::PcmS16),
            session_id: session.id,
            layout: SegmentLayout::under_data_dir(&data_dir),
        },
        receiver,
    )
    .expect("recorder");

    let mut noise = Noise(0x9e37_79b9_7f4a_7c15);
    let frames = SCENE_SECONDS * 1000 / FRAME_MS;
    for frame in 0..frames {
        let samples: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|index| {
                let sample_index = frame as usize * FRAME_SAMPLES + index;
                let seconds = sample_index as f64 / f64::from(RATE);
                let value = scene(seconds, &mut noise) * gain;
                (value.clamp(-1.0, 1.0) * f64::from(i16::MAX)).round() as i16
            })
            .collect();
        let timestamp_ms = T0 + frame * FRAME_MS;
        sender
            .send(AudioFrame::from_samples(
                timestamp_ms,
                RATE,
                1,
                samples,
                true,
            ))
            .expect("the recorder takes every frame");
    }
    drop(sender);
    // Stopping closes and indexes the segment in progress, as shutting the service down does.
    recorder.stop();

    Recorded {
        timeline: TimelineService::new(segments, hub),
        data_dir,
    }
}

fn seconds_into_scene(ms: i64) -> f64 {
    (ms - T0) as f64 / 1000.0
}

#[test]
fn recorded_audio_is_indexed_with_its_envelope() {
    let recorded = record_scene("indexed", 1.0);
    let range = recorded.timeline.range().expect("range");
    assert_eq!(range.earliest_ms, Some(T0));
    assert_eq!(range.latest_ms, Some(T0 + SCENE_SECONDS * 1000));
    // The envelope the timeline draws shows the door far louder than the room.
    let view = recorded
        .timeline
        .peaks(T0, T0 + SCENE_SECONDS * 1000, 1800)
        .expect("peaks");
    let at = |seconds: f64| view.values[(seconds * 10.0) as usize];
    assert!(
        at(30.0) <= 2,
        "the room reads as near silence, got {}",
        at(30.0)
    );
    assert!(
        at(DOOR_AT_S) > 50,
        "the door is loud, got {}",
        at(DOOR_AT_S)
    );
}

#[test]
fn speech_and_a_door_are_found_and_the_click_and_the_hum_are_not() {
    let recorded = record_scene("normal", 1.0);

    for sensitivity in [SoundSensitivity::Medium, SoundSensitivity::High] {
        let sounds = recorded
            .timeline
            .sounds(T0, T0 + SCENE_SECONDS * 1000, sensitivity)
            .expect("sounds");
        let found: Vec<(f64, f64)> = sounds
            .iter()
            .map(|sound| {
                (
                    seconds_into_scene(sound.start_ms),
                    seconds_into_scene(sound.end_ms),
                )
            })
            .collect();
        assert_eq!(sounds.len(), 2, "{sensitivity:?} found {found:?}");

        let (speech, door) = (&sounds[0], &sounds[1]);
        // The syllables and the pauses between them are one sound, starting with the first syllable.
        assert!(
            (seconds_into_scene(speech.start_ms) - SPEECH_FROM_S).abs() <= 0.1,
            "{found:?}"
        );
        assert!(
            (seconds_into_scene(speech.end_ms) - SPEECH_TO_S).abs() <= 0.5,
            "{found:?}"
        );
        assert!(
            (seconds_into_scene(door.start_ms) - DOOR_AT_S).abs() <= 0.1,
            "{found:?}"
        );
        assert!(
            door.end_ms - door.start_ms <= 1_000,
            "the thud dies away: {found:?}"
        );
        assert!(door.peak > speech.peak);
        // Playback starts a second before each.
        assert_eq!(speech.seek_ms, speech.start_ms - 1_000);
    }

    // And the buttons walk them: from the start, next is the speech, then the door, then nothing.
    use crate::services::timeline_service::SeekDirection::{Backward, Forward};
    let medium = SoundSensitivity::Medium;
    let first = recorded
        .timeline
        .next_sound(T0, Forward, medium)
        .expect("next")
        .expect("speech");
    let second = recorded
        .timeline
        .next_sound(first.seek_ms + 300, Forward, medium)
        .expect("next")
        .expect("door");
    assert!((seconds_into_scene(second.start_ms) - DOOR_AT_S).abs() <= 0.1);
    assert_eq!(
        recorded
            .timeline
            .next_sound(second.seek_ms + 300, Forward, medium)
            .expect("next"),
        None
    );
    let back = recorded
        .timeline
        .next_sound(T0 + SCENE_SECONDS * 1000, Backward, medium)
        .expect("previous")
        .expect("door");
    assert_eq!(back.start_ms, second.start_ms);
}

/// The start of every sound found at `sensitivity`, in seconds into the scene.
fn starts(recorded: &Recorded, sensitivity: SoundSensitivity) -> Vec<f64> {
    recorded
        .timeline
        .sounds(T0, T0 + SCENE_SECONDS * 1000, sensitivity)
        .expect("sounds")
        .iter()
        .map(|sound| seconds_into_scene(sound.start_ms))
        .collect()
}

#[test]
fn at_a_tenth_of_the_gain_speech_falls_below_what_the_levels_can_resolve() {
    // The limitation the user guide states: levels are kept on a coarse scale, one step being about
    // -48 dBFS, so a quiet voice on a quiet microphone is lost at any sensitivity. Even the door, still
    // visible in the waveform, rises for too short a moment to count.
    let quiet = record_scene("tenth", 0.1);
    for sensitivity in [
        SoundSensitivity::Low,
        SoundSensitivity::Medium,
        SoundSensitivity::High,
    ] {
        assert!(starts(&quiet, sensitivity).is_empty(), "{sensitivity:?}");
    }
    let view = quiet
        .timeline
        .peaks(T0, T0 + SCENE_SECONDS * 1000, 1800)
        .expect("peaks");
    let speech = &view.values[(SPEECH_FROM_S * 10.0) as usize..(SPEECH_TO_S * 10.0) as usize];
    assert!(speech.iter().all(|level| *level <= 1), "{speech:?}");
    assert!(view.values[(DOOR_AT_S * 10.0) as usize] >= 5);
}

#[test]
fn a_quiet_voice_is_found_again_by_choosing_high_or_raising_the_gain() {
    // The advice the settings page and the user guide give, shown on real samples: the same voice a little
    // above the coarse limit is missed at Medium, found at High, and found at Medium once the gain is up.
    let quietish = record_scene("third", 0.35);
    let medium = starts(&quietish, SoundSensitivity::Medium);
    assert!(
        !medium
            .iter()
            .any(|start| (start - SPEECH_FROM_S).abs() <= 0.5),
        "Medium should miss the quiet voice: {medium:?}"
    );
    let high = starts(&quietish, SoundSensitivity::High);
    assert!(
        high.iter()
            .any(|start| (start - SPEECH_FROM_S).abs() <= 0.1),
        "High should find it: {high:?}"
    );

    let louder = record_scene("half", 0.5);
    let medium = starts(&louder, SoundSensitivity::Medium);
    assert!(
        medium
            .iter()
            .any(|start| (start - SPEECH_FROM_S).abs() <= 0.1),
        "Medium should find it with more gain: {medium:?}"
    );
}
