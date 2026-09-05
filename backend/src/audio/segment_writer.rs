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
use crate::util::day::local_day;

/// Where segments are written, and how their paths are recorded.
///
/// Two roots rather than one, because the file location and the indexed path are different questions.
/// Recordings in the default place are indexed relative to the data directory so the whole directory
/// stays relocatable; recordings the operator has redirected elsewhere are indexed absolutely, because a
/// relative path would have nothing meaningful to be relative to.
#[derive(Debug, Clone)]
pub struct SegmentLayout {
    /// Absolute directory segments are written into.
    pub recordings_dir: PathBuf,
    /// Root the stored path is made relative to. `None` stores absolute paths.
    pub relative_to: Option<PathBuf>,
}

impl SegmentLayout {
    /// The default layout: `<data dir>/recordings`, indexed relative to the data directory.
    pub fn under_data_dir(data_dir: &Path) -> Self {
        Self {
            recordings_dir: data_dir.join("recordings"),
            relative_to: Some(data_dir.to_path_buf()),
        }
    }

    /// A layout writing into `recordings_dir`, indexed absolutely.
    pub fn at(recordings_dir: PathBuf) -> Self {
        Self {
            recordings_dir,
            relative_to: None,
        }
    }
}

/// Where a segment lives, on disk and on the calendar.
///
/// This is the one place the storage layout is decided. Deriving the day and the path together from the
/// same timestamp is what guarantees a segment's `day` column always names the directory its file is
/// actually in, which is the property the day picker relies on.
#[derive(Debug, Clone)]
pub struct SegmentLocation {
    pub session_id: i64,
    pub sequence: i64,
    /// Local calendar day, `YYYY-MM-DD`.
    pub day: String,
    /// Path relative to the data directory, which is what the index stores.
    pub relative_path: String,
    pub absolute_path: PathBuf,
}

impl SegmentLocation {
    /// Lay out the segment that starts at `started_at_ms`.
    ///
    /// Recordings are grouped by day first and session second, so a day's audio is one directory even
    /// when the recorder was stopped and started several times within it. That ordering is what makes
    /// the data directory browsable by hand and a day's material trivial to archive or delete.
    pub fn for_segment(
        layout: &SegmentLayout,
        session_id: i64,
        sequence: i64,
        started_at_ms: i64,
    ) -> Self {
        let day = local_day(started_at_ms);
        let absolute_path = layout
            .recordings_dir
            .join(&day)
            .join(session_id.to_string())
            .join(format!("{sequence:06}.pcm"));

        // Separators are normalised so an index written on Windows still reads on any other platform.
        let relative_path = layout
            .relative_to
            .as_ref()
            .and_then(|root| absolute_path.strip_prefix(root).ok())
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| absolute_path.to_string_lossy().to_string());

        Self {
            session_id,
            sequence,
            day,
            relative_path,
            absolute_path,
        }
    }
}

pub struct SegmentWriter {
    session_id: i64,
    sequence: i64,
    day: String,
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
    pub fn create(location: SegmentLocation, first_frame: &AudioFrame) -> AppResult<Self> {
        if let Some(parent) = location.absolute_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = File::create(&location.absolute_path)?;

        Ok(Self {
            session_id: location.session_id,
            sequence: location.sequence,
            day: location.day,
            absolute_path: location.absolute_path,
            relative_path: location.relative_path,
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
            day: self.day,
            path: self.relative_path,
            started_at_ms: self.started_at_ms,
            ended_at_ms: self.last_end_ms,
            sample_rate: self.sample_rate,
            channels: self.channels,
            byte_len: self.byte_len,
            peaks: self.envelope.finish(),
        }))
    }

    /// Local calendar day this segment belongs to.
    pub fn day(&self) -> &str {
        &self.day
    }
}

impl std::fmt::Debug for SegmentWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SegmentWriter")
            .field("session_id", &self.session_id)
            .field("sequence", &self.sequence)
            .field("day", &self.day)
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
        AppError::internal(format!(
            "could not open segment {}: {error}",
            path.display()
        ))
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
        let first = AudioFrame::from_samples(1_000, 48_000, 1, vec![0; 4800], true);
        let location =
            SegmentLocation::for_segment(&SegmentLayout::at(dir.clone()), 1, 0, first.timestamp_ms);

        let mut writer = SegmentWriter::create(location, &first).expect("create");
        writer.append(&first, &PcmS16Encoder).expect("append");

        let second = AudioFrame::from_samples(1_100, 48_000, 1, vec![1000; 4800], true);
        writer.append(&second, &PcmS16Encoder).expect("append");

        let draft = writer.finish().expect("finish").expect("indexed");
        assert_eq!(draft.day, crate::util::day::local_day(1_000));
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
        let frame = AudioFrame::from_samples(0, 48_000, 1, vec![0; 10], true);
        let location =
            SegmentLocation::for_segment(&SegmentLayout::at(dir.clone()), 1, 1, frame.timestamp_ms);
        let path = location.absolute_path.clone();

        let writer = SegmentWriter::create(location, &frame).expect("create");
        assert!(writer.finish().expect("finish").is_none());
        assert!(!path.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_back_the_bytes_that_were_written() {
        let dir = temp_dir("read");
        let frame = AudioFrame::from_samples(0, 48_000, 1, vec![1, 2, 3, 4], true);
        let location =
            SegmentLocation::for_segment(&SegmentLayout::at(dir.clone()), 1, 2, frame.timestamp_ms);
        let path = location.absolute_path.clone();

        let mut writer = SegmentWriter::create(location, &frame).expect("create");
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
    fn segments_are_laid_out_by_day_then_session() {
        let started_at_ms = 1_757_030_400_000;
        let day = crate::util::day::local_day(started_at_ms);
        let layout = SegmentLayout::under_data_dir(Path::new("/srv/oar"));
        let location = SegmentLocation::for_segment(&layout, 3, 12, started_at_ms);

        assert_eq!(location.day, day);
        assert_eq!(
            location.relative_path,
            format!("recordings/{day}/3/000012.pcm")
        );
        assert_eq!(
            location.absolute_path,
            PathBuf::from(format!("/srv/oar/recordings/{day}/3/000012.pcm"))
        );
    }

    #[test]
    fn a_redirected_directory_is_indexed_absolutely() {
        let started_at_ms = 1_757_030_400_000;
        let day = crate::util::day::local_day(started_at_ms);
        let layout = SegmentLayout::at(PathBuf::from("/mnt/audio"));
        let location = SegmentLocation::for_segment(&layout, 3, 12, started_at_ms);

        // No `recordings` prefix: the chosen directory is itself the recordings root.
        assert_eq!(
            location.absolute_path,
            PathBuf::from(format!("/mnt/audio/{day}/3/000012.pcm"))
        );
        // Absolute, because there is no root it could sensibly be relative to.
        assert_eq!(
            location.relative_path,
            location.absolute_path.to_string_lossy()
        );
    }

    #[test]
    fn two_sessions_on_one_day_share_the_day_directory() {
        let started_at_ms = 1_757_030_400_000;
        let layout = SegmentLayout::under_data_dir(Path::new("/srv/oar"));
        let morning = SegmentLocation::for_segment(&layout, 1, 0, started_at_ms);
        let evening = SegmentLocation::for_segment(&layout, 2, 0, started_at_ms + 3_600_000);

        assert_eq!(morning.day, evening.day);
        assert_ne!(morning.relative_path, evening.relative_path);
    }
}
