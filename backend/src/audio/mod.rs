//! Everything that touches the host audio system or the raw sample stream.
//!
//! This module is the only place that knows about `cpal`. Services above it work with [`AudioFrame`]
//! values and never see a device handle, which is what keeps the rest of the codebase portable and
//! testable without hardware.
//!
//! [`AudioFrame`]: crate::models::AudioFrame

pub mod capture;
pub mod device_registry;
pub mod encoder;
pub mod frame_builder;
pub mod peaks;
pub mod resampler;
pub mod segment_writer;

pub use capture::{CaptureHandle, CaptureOptions, CaptureRuntime, GainControl};
pub use device_registry::DeviceRegistry;
pub use encoder::{build_encoder, FrameEncoder, FrameFormat, PcmS16Encoder};
pub use frame_builder::FrameBuilder;
pub use peaks::{PeakEnvelopeBuilder, PeakSource, PEAK_BUCKET_MS};
pub use resampler::Resampler;
pub use segment_writer::{SegmentLayout, SegmentLocation, SegmentWriter};
