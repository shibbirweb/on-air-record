//! Writes one segment file and produces its index entry.
//!
//! The writer owns exactly one file at a time. Rolling over to the next segment is the recorder's job, so
//! this type stays a simple, testable sink with no notion of time policy.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::audio::peaks::PeakEnvelopeBuilder;
use crate::audio::FrameEncoder;
use crate::error::{AppError, AppResult};
use crate::models::{AudioFrame, SegmentDraft};

pub struct SegmentWriter {
    session_id: i64,
    sequence: i64,
    absolute_path: PathBuf,
    relative_path: String,
    writer: BufWriter<File>,
    started_at_ms: i64,
    last_end_ms: i64,
    byte_len: i64,
    sample_rate: u32,
    channels: u16,
    envelope: PeakEnvelopeBuilder,
}

impl SegmentWriter {
    /// Create the segment file and prepare its envelope accumulator.
    pub fn create(
        session_id: i64,
        sequence: i64,
        absolute_path: PathBuf,
        relative_path: String,
        first_frame: &AudioFrame,
    ) -> AppResult<Self> {
        if let Some(parent) = absolute_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = File::create(&absolute_path)?;

        Ok(Self {
            session_id,
            sequence,
            absolute_path,
            relative_path,
            // 64 KiB is about seven frames at the default settings, so the recorder touches the disk once
            // per second rather than ten times.
            writer: BufWriter::with_capacity(64 * 1024, file),
            started_at_ms: first_frame.timestamp_ms,
            last_end_ms: first_frame.timestamp_ms,
            byte_len: 0,
            sample_rate: first_frame.sample_rate,
            channels: first_frame.channels,
            envelope: PeakEnvelopeBuilder::new(first_frame.sample_rate),
        })
    }

    pub fn started_at_ms(&self) -> i64 {
        self.started_at_ms
    }

    pub fn duration_ms(&self) -> i64 {
        (self.last_end_ms - self.started_at_ms).max(0)
    }

    pub fn byte_len(&self) -> i64 {
        self.byte_len
    }

    pub fn path(&self) -> &Path {
        &self.absolute_path
    }

    /// Append a frame's payload and fold it into the envelope.
    pub fn append(&mut self, frame: &AudioFrame, encoder: &dyn FrameEncoder) -> AppResult<()> {
        let payload = encoder.encode(frame);
        self.writer.write_all(&payload)?;
        self.byte_len += payload.len() as i64;
        self.envelope.push(&frame.samples);
        self.last_end_ms = frame.end_timestamp_ms();
        Ok(())
    }

    /// Flush, close, and describe the segment so it can be indexed.
    ///
    /// A segment that never received a frame is removed instead, because an empty file in the index would
    /// make playback stall on a zero length read.
    pub fn finish(mut self) -> AppResult<Option<SegmentDraft>> {
        self.writer.flush()?;
        drop(self.writer);

        if self.byte_len == 0 {
            if let Err(error) = std::fs::remove_file(&self.absolute_path) {
                tracing::warn!(path = %self.absolute_path.display(), %error, "could not remove empty segment");
            }
            return Ok(None);
        }

        Ok(Some(SegmentDraft {
            session_id: self.session_id,
            sequence: self.sequence,
            path: self.relative_path,
            started_at_ms: self.started_at_ms,
            ended_at_ms: self.last_end_ms,
            sample_rate: self.sample_rate,
            channels: self.channels,
            byte_len: self.byte_len,
            peaks: self.envelope.finish(),
        }))
    }

    /// Build the conventional path of a segment inside the data directory.
    pub fn relative_path_for(session_id: i64, sequence: i64) -> String {
        format!("recordings/{session_id}/{sequence:06}.pcm")
    }
}

impl std::fmt::Debug for SegmentWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SegmentWriter")
            .field("session_id", &self.session_id)
            .field("sequence", &self.sequence)
            .field("path", &self.relative_path)
            .field("byte_len", &self.byte_len)
            .finish()
    }
}

/// Read a byte range out of a segment file.
///
/// Kept next to the writer so the two halves of the on disk format stay in one place.
pub fn read_segment_bytes(path: &Path, offset: i64, length: usize) -> AppResult<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = File::open(path).map_err(|error| {
        AppError::internal(format!("could not open segment {}: {error}", path.display()))
    })?;
    file.seek(SeekFrom::Start(offset.max(0) as u64))?;

    let mut buffer = vec![0u8; length];
    let mut filled = 0;
    while filled < length {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    buffer.truncate(filled);
    Ok(buffer)
}

/// Interpret raw little endian PCM bytes as signed 16 bit samples.
pub fn decode_pcm_s16(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::PcmS16Encoder;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oar-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn writes_frames_and_reports_the_range() {
        let dir = temp_dir("writer");
        let path = dir.join("000000.pcm");
        let first = AudioFrame::from_samples(1_000, 48_000, 1, vec![0; 4800], true);

        let mut writer = SegmentWriter::create(
            1,
            0,
            path.clone(),
            "recordings/1/000000.pcm".to_string(),
            &first,
        )
        .expect("create");
        writer.append(&first, &PcmS16Encoder).expect("append");

        let second = AudioFrame::from_samples(1_100, 48_000, 1, vec![1000; 4800], true);
        writer.append(&second, &PcmS16Encoder).expect("append");

        let draft = writer.finish().expect("finish").expect("indexed");
        assert_eq!(draft.started_at_ms, 1_000);
        assert_eq!(draft.ended_at_ms, 1_200);
        assert_eq!(draft.byte_len, 4800 * 2 * 2);
        assert_eq!(draft.peaks.len(), 2);
        assert_eq!(draft.peaks[0], 0);
        assert!(draft.peaks[1] > 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_segment_is_discarded() {
        let dir = temp_dir("empty");
        let path = dir.join("000001.pcm");
        let frame = AudioFrame::from_samples(0, 48_000, 1, vec![0; 10], true);

        let writer =
            SegmentWriter::create(1, 1, path.clone(), "recordings/1/000001.pcm".to_string(), &frame)
                .expect("create");
        assert!(writer.finish().expect("finish").is_none());
        assert!(!path.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_back_the_bytes_that_were_written() {
        let dir = temp_dir("read");
        let path = dir.join("000002.pcm");
        let frame = AudioFrame::from_samples(0, 48_000, 1, vec![1, 2, 3, 4], true);

        let mut writer =
            SegmentWriter::create(1, 2, path.clone(), "recordings/1/000002.pcm".to_string(), &frame)
                .expect("create");
        writer.append(&frame, &PcmS16Encoder).expect("append");
        writer.finish().expect("finish");

        let bytes = read_segment_bytes(&path, 2, 4).expect("read");
        assert_eq!(decode_pcm_s16(&bytes), vec![2, 3]);

        // Reading past the end returns what exists rather than failing.
        let tail = read_segment_bytes(&path, 6, 100).expect("read tail");
        assert_eq!(decode_pcm_s16(&tail), vec![4]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn segment_paths_are_zero_padded() {
        assert_eq!(
            SegmentWriter::relative_path_for(3, 12),
            "recordings/3/000012.pcm"
        );
    }
}
