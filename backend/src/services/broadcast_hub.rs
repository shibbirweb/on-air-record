//! Fan out point for live audio.
//!
//! One publisher (the recorder thread) and any number of subscribers (WebSocket sessions). This is the
//! observer pattern with a tokio broadcast channel doing the bookkeeping, and it is the reason adding a
//! new consumer of live audio never means editing the capture path.
//!
//! Slow subscribers are the interesting case. A broadcast channel keeps a bounded ring per channel, not
//! per subscriber, so a listener that stops reading does not grow memory and cannot stall the publisher.
//! It simply misses frames and is told how many by `RecvError::Lagged`, which each session handles by
//! resynchronising rather than disconnecting.

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};

use tokio::sync::broadcast;

use crate::models::{AudioFrame, LevelSnapshot};

/// Frames buffered for subscribers that fall behind. At the default 100 ms frame this is six seconds of
/// slack, which absorbs a garbage collection pause or a Wi-Fi hiccup without dropping anyone.
pub const BROADCAST_CAPACITY: usize = 64;

pub struct BroadcastHub {
    sender: broadcast::Sender<AudioFrame>,
    /// Newest captured timestamp, which is what "live" means to a seeking client.
    live_edge_ms: AtomicI64,
    /// Input meter, stored as `f32` bit patterns so readers never take a lock.
    rms_bits: AtomicU32,
    peak_bits: AtomicU32,
    frames_published: AtomicU64,
}

impl BroadcastHub {
    pub fn new() -> Self {
        let (sender, _receiver) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            sender,
            live_edge_ms: AtomicI64::new(0),
            rms_bits: AtomicU32::new(0),
            peak_bits: AtomicU32::new(0),
            frames_published: AtomicU64::new(0),
        }
    }

    /// Publish a captured frame. Callable from any thread, including the recorder thread.
    ///
    /// A send with no subscribers is not an error: the recorder keeps recording whether or not anybody is
    /// listening, which is the whole point of a DVR.
    pub fn publish(&self, frame: AudioFrame) {
        self.live_edge_ms
            .store(frame.end_timestamp_ms(), Ordering::Relaxed);
        self.rms_bits.store(frame.rms.to_bits(), Ordering::Relaxed);
        self.peak_bits
            .store(frame.peak.to_bits(), Ordering::Relaxed);
        self.frames_published.fetch_add(1, Ordering::Relaxed);
        let _ = self.sender.send(frame);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AudioFrame> {
        self.sender.subscribe()
    }

    /// Number of connected live subscribers.
    pub fn listener_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Newest captured timestamp, or `None` before the first frame of the process.
    pub fn live_edge_ms(&self) -> Option<i64> {
        match self.live_edge_ms.load(Ordering::Relaxed) {
            0 => None,
            value => Some(value),
        }
    }

    pub fn levels(&self) -> LevelSnapshot {
        LevelSnapshot {
            rms: f32::from_bits(self.rms_bits.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.peak_bits.load(Ordering::Relaxed)),
        }
    }

    pub fn frames_published(&self) -> u64 {
        self.frames_published.load(Ordering::Relaxed)
    }

    /// Zero the meter when capture stops, so the UI does not keep showing the last level forever.
    pub fn reset_levels(&self) {
        self.rms_bits.store(0, Ordering::Relaxed);
        self.peak_bits.store(0, Ordering::Relaxed);
    }
}

impl Default for BroadcastHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(timestamp_ms: i64) -> AudioFrame {
        AudioFrame::from_samples(timestamp_ms, 48_000, 1, vec![1000; 4800], true)
    }

    #[tokio::test]
    async fn subscribers_receive_published_frames() {
        let hub = BroadcastHub::new();
        let mut first = hub.subscribe();
        let mut second = hub.subscribe();
        assert_eq!(hub.listener_count(), 2);

        hub.publish(frame(1_000));

        assert_eq!(first.recv().await.expect("first").timestamp_ms, 1_000);
        assert_eq!(second.recv().await.expect("second").timestamp_ms, 1_000);
    }

    #[tokio::test]
    async fn publishing_without_subscribers_is_not_an_error() {
        let hub = BroadcastHub::new();
        hub.publish(frame(2_000));
        assert_eq!(hub.frames_published(), 1);
        assert_eq!(hub.live_edge_ms(), Some(2_100));
    }

    #[tokio::test]
    async fn live_edge_tracks_the_end_of_the_newest_frame() {
        let hub = BroadcastHub::new();
        assert_eq!(hub.live_edge_ms(), None);

        hub.publish(frame(5_000));
        assert_eq!(hub.live_edge_ms(), Some(5_100));
    }

    #[tokio::test]
    async fn levels_follow_the_newest_frame_and_reset_on_demand() {
        let hub = BroadcastHub::new();
        hub.publish(frame(1_000));
        assert!(hub.levels().rms > 0.0);

        hub.reset_levels();
        assert_eq!(hub.levels().rms, 0.0);
        assert_eq!(hub.levels().peak, 0.0);
    }

    #[tokio::test]
    async fn a_slow_subscriber_lags_instead_of_stalling_the_publisher() {
        let hub = BroadcastHub::new();
        let mut receiver = hub.subscribe();

        for index in 0..(BROADCAST_CAPACITY as i64 + 10) {
            hub.publish(frame(index * 100));
        }

        match receiver.recv().await {
            Err(broadcast::error::RecvError::Lagged(missed)) => assert!(missed > 0),
            other => panic!("expected a lag notification, got {other:?}"),
        }
    }
}
