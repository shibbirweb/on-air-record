//! The capture engine: opens a host input stream and turns it into a frame feed.
//!
//! `cpal::Stream` is not `Send` on every backend, so the stream lives on a dedicated OS thread for its
//! whole life. That thread does nothing but hold the stream open and watch a stop flag; all real work
//! happens on the driver's own callback thread. The async runtime never touches either one, it only sees
//! the frames arriving on a channel.
//!
//! Rules the callback obeys, because breaking any of them causes audible glitches:
//!
//! - never block on the network, the database, or a lock another subsystem holds,
//! - never allocate in a loop, and
//! - never panic, since unwinding out of a C callback is undefined behaviour.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use crossbeam_channel::{Sender, TrySendError};

use crate::audio::frame_builder::FrameBuilder;
use crate::audio::DeviceRegistry;
use crate::error::{AppError, AppResult};
use crate::models::AudioFrame;
use crate::util::time::now_ms;

/// Input gain shared between the settings service and the audio callback.
///
/// An atomic rather than a lock, because the callback reads it once per buffer and must never wait for a
/// writer. The value is stored as the bit pattern of an `f32`, which is the standard way to keep a float
/// in an atomic without a lock.
#[derive(Debug, Clone)]
pub struct GainControl {
    bits: Arc<AtomicU32>,
}

impl GainControl {
    pub fn new(gain: f32) -> Self {
        Self {
            bits: Arc::new(AtomicU32::new(gain.to_bits())),
        }
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.bits.load(Ordering::Relaxed))
    }

    pub fn set(&self, gain: f32) {
        let sanitised = if gain.is_finite() { gain.clamp(0.0, 4.0) } else { 1.0 };
        self.bits.store(sanitised.to_bits(), Ordering::Relaxed);
    }
}

impl Default for GainControl {
    fn default() -> Self {
        Self::new(1.0)
    }
}

pub struct CaptureOptions {
    /// `None` follows the system default input.
    pub device_id: Option<String>,
    pub frame_ms: u32,
    pub gain: GainControl,
    /// Counter the callback bumps when the consumer cannot keep up. Owned by the caller so the count
    /// survives a capture restart and stays visible in the status endpoint.
    pub dropped_frames: Arc<AtomicU64>,
}

/// What the engine actually negotiated with the device.
#[derive(Debug, Clone)]
pub struct CaptureRuntime {
    pub device_id: String,
    pub device_name: String,
    /// Sample rate of the captured stream. The device decides, we do not resample.
    pub sample_rate: u32,
    /// Channels after the downmix, always 1 today.
    pub channels: u16,
    /// Channels the device delivered before the downmix, kept for diagnostics.
    pub source_channels: u16,
    pub frame_ms: u32,
}

/// Live handle on a running capture. Dropping it stops the stream.
pub struct CaptureHandle {
    runtime: CaptureRuntime,
    stop: Arc<AtomicBool>,
    dropped_frames: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn runtime(&self) -> &CaptureRuntime {
        &self.runtime
    }

    /// Frames the consumer was too slow to accept. A non zero value means the recorder or the runtime is
    /// starved, and it is surfaced in the status endpoint rather than only logged.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    /// Stop the stream and wait for the capture thread to unwind.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::error!("capture thread panicked while shutting down");
            }
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// How long the supervising thread sleeps between stop flag checks.
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Open the configured input and start delivering frames to `sink`.
///
/// Returns once the device is open and producing, or with the host error if it is not, so the caller can
/// report a precise failure instead of a capture that silently never starts.
pub fn spawn(options: CaptureOptions, sink: Sender<AudioFrame>) -> AppResult<CaptureHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let dropped_frames = options.dropped_frames.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<AppResult<CaptureRuntime>>();

    let thread_stop = stop.clone();
    let thread_dropped = dropped_frames.clone();

    let thread = std::thread::Builder::new()
        .name("oar-capture".to_string())
        .spawn(move || {
            run_capture_thread(options, sink, ready_tx, thread_stop, thread_dropped);
        })
        .map_err(|error| AppError::internal(format!("could not start capture thread: {error}")))?;

    // The device open happens on the capture thread, so wait for its verdict before reporting success.
    let runtime = match ready_rx.recv() {
        Ok(Ok(runtime)) => runtime,
        Ok(Err(error)) => {
            let _ = thread.join();
            return Err(error);
        }
        Err(_) => {
            let _ = thread.join();
            return Err(AppError::audio("capture thread stopped before it started"));
        }
    };

    Ok(CaptureHandle {
        runtime,
        stop,
        dropped_frames,
        thread: Some(thread),
    })
}

fn run_capture_thread(
    options: CaptureOptions,
    sink: Sender<AudioFrame>,
    ready: std::sync::mpsc::Sender<AppResult<CaptureRuntime>>,
    stop: Arc<AtomicBool>,
    dropped_frames: Arc<AtomicU64>,
) {
    let started = build_stream(&options, sink, dropped_frames);

    let (stream, runtime, builder) = match started {
        Ok(parts) => parts,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };

    if let Err(error) = stream.play() {
        let _ = ready.send(Err(AppError::audio(format!("could not start the input stream: {error}"))));
        return;
    }

    tracing::info!(
        device = %runtime.device_name,
        sample_rate = runtime.sample_rate,
        source_channels = runtime.source_channels,
        frame_ms = runtime.frame_ms,
        "capture started"
    );

    let _ = ready.send(Ok(runtime));

    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(STOP_POLL);
    }

    // Dropping the stream first guarantees the callback is no longer running, so the flush below cannot
    // race with it and the tail of the recording is emitted exactly once.
    drop(stream);

    if let Ok(mut builder) = builder.lock() {
        builder.flush(now_ms(), |_frame| {
            // The sink may already be gone when the service is shutting down, and a lost partial frame is
            // not worth propagating an error for.
        });
    }

    tracing::info!("capture stopped");
}

type StreamParts = (cpal::Stream, CaptureRuntime, Arc<Mutex<FrameBuilder>>);

fn build_stream(
    options: &CaptureOptions,
    sink: Sender<AudioFrame>,
    dropped_frames: Arc<AtomicU64>,
) -> AppResult<StreamParts> {
    let (device, supported, descriptor) = DeviceRegistry::resolve(options.device_id.as_deref())?;

    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.config();
    let sample_rate = config.sample_rate.0;
    let source_channels = config.channels.max(1);

    let builder = Arc::new(Mutex::new(FrameBuilder::new(sample_rate, 1, options.frame_ms)));

    let stream = match sample_format {
        SampleFormat::I8 => open::<i8>(&device, &config, options, &builder, &sink, &dropped_frames),
        SampleFormat::I16 => open::<i16>(&device, &config, options, &builder, &sink, &dropped_frames),
        SampleFormat::I32 => open::<i32>(&device, &config, options, &builder, &sink, &dropped_frames),
        SampleFormat::U8 => open::<u8>(&device, &config, options, &builder, &sink, &dropped_frames),
        SampleFormat::U16 => open::<u16>(&device, &config, options, &builder, &sink, &dropped_frames),
        SampleFormat::U32 => open::<u32>(&device, &config, options, &builder, &sink, &dropped_frames),
        SampleFormat::F32 => open::<f32>(&device, &config, options, &builder, &sink, &dropped_frames),
        SampleFormat::F64 => open::<f64>(&device, &config, options, &builder, &sink, &dropped_frames),
        other => Err(AppError::audio(format!(
            "device '{}' uses the unsupported sample format {other:?}",
            descriptor.name
        ))),
    }?;

    let runtime = CaptureRuntime {
        device_id: descriptor.id,
        device_name: descriptor.name,
        sample_rate,
        channels: 1,
        source_channels,
        frame_ms: options.frame_ms,
    };

    Ok((stream, runtime, builder))
}

fn open<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    options: &CaptureOptions,
    builder: &Arc<Mutex<FrameBuilder>>,
    sink: &Sender<AudioFrame>,
    dropped_frames: &Arc<AtomicU64>,
) -> AppResult<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    let gain = options.gain.clone();
    let builder = builder.clone();
    let sink = sink.clone();
    let dropped_frames = dropped_frames.clone();

    // Reused across callbacks so the hot path never grows a buffer.
    let mut mono = Vec::<i16>::with_capacity(4096);

    let data_callback = move |input: &[T], _info: &cpal::InputCallbackInfo| {
        let gain_value = gain.get();
        mono.clear();
        mono.reserve(input.len() / channels + 1);

        for chunk in input.chunks(channels) {
            let mut sum = 0.0f32;
            for sample in chunk {
                sum += f32::from_sample(*sample);
            }
            let averaged = (sum / chunk.len() as f32) * gain_value;
            mono.push(to_i16(averaged));
        }

        let captured_at_ms = now_ms();
        if let Ok(mut builder) = builder.lock() {
            builder.push(&mono, captured_at_ms, |frame| {
                match sink.try_send(frame) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        dropped_frames.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        dropped_frames.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    };

    let error_callback = |error: cpal::StreamError| {
        tracing::error!(%error, "input stream error");
    };

    device
        .build_input_stream(config, data_callback, error_callback, None)
        .map_err(|error| AppError::audio(format!("could not open the input stream: {error}")))
}

/// Convert a normalised float sample to signed 16 bit, clamping rather than wrapping.
///
/// Wrapping on overload is what turns a slightly hot microphone into harsh digital noise, so clipping is
/// the deliberate choice here.
fn to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_is_shared_and_clamped() {
        let gain = GainControl::new(1.0);
        let clone = gain.clone();

        clone.set(2.5);
        assert_eq!(gain.get(), 2.5);

        clone.set(99.0);
        assert_eq!(gain.get(), 4.0);

        clone.set(f32::NAN);
        assert_eq!(gain.get(), 1.0);
    }

    #[test]
    fn float_samples_clip_instead_of_wrapping() {
        assert_eq!(to_i16(0.0), 0);
        assert_eq!(to_i16(1.0), i16::MAX);
        assert_eq!(to_i16(-1.0), -i16::MAX);
        assert_eq!(to_i16(5.0), i16::MAX);
        assert_eq!(to_i16(-5.0), -i16::MAX);
    }
}
