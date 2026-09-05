/**
 * The visible timeline window, its coverage bands and its waveform.
 *
 * The window is the state that matters: everything drawn is a function of it. Peaks are fetched for the
 * window rather than for all of history, which is what keeps an eight hour recording as cheap to draw as
 * an eight minute one.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { CoverageBand, Peaks, RecordingDay, TimelineRange } from '@/api/types';
import { zoomWindow } from '@/lib/timelineGeometry';

/** Selectable zoom levels, in milliseconds of visible span. */
export const ZOOM_LEVELS = [
  60_000,
  5 * 60_000,
  15 * 60_000,
  60 * 60_000,
  4 * 60 * 60_000,
  12 * 60 * 60_000,
  24 * 60 * 60_000,
] as const;

export const DEFAULT_SPAN_MS = ZOOM_LEVELS[2];

/** Zoom limits. Ten seconds shows individual words; a week is past the point of being readable. */
const MIN_SPAN_MS = 10_000;
const MAX_SPAN_MS = 7 * 24 * 3_600_000;

/** Columns requested per fetch. More than a wide screen has pixels is wasted bandwidth. */
const PEAK_BUCKETS = 1200;

type TimelineState = {
  /** Left edge of the visible window, epoch milliseconds. */
  windowStartMs: number;
  spanMs: number;
  /** True while the window should scroll to keep the live edge at the right. */
  followingLive: boolean;
  range: TimelineRange | null;
  peaks: Peaks | null;
  /** Calendar days that hold recordings, newest first. */
  days: RecordingDay[];
  loadingPeaks: boolean;
  error: string | null;

  windowEndMs: () => number;
  coverage: () => CoverageBand[];
  /** The day entry the visible window sits in, if any. */
  activeDay: () => RecordingDay | null;

  /** Rescale the window, holding `anchorMs` in place. Falls back to the centre when no anchor is given. */
  zoomTo: (spanMs: number, anchorMs?: number | null) => void;
  panBy: (deltaMs: number) => void;
  centreOn: (timestampMs: number) => void;
  setFollowingLive: (following: boolean) => void;
  showDay: (day: string) => void;
  refreshRange: () => Promise<void>;
  refreshPeaks: () => Promise<void>;
  refreshDays: () => Promise<void>;
};

export const useTimelineStore = create<TimelineState>((set, get) => ({
  windowStartMs: Date.now() - DEFAULT_SPAN_MS,
  spanMs: DEFAULT_SPAN_MS,
  followingLive: true,
  range: null,
  peaks: null,
  days: [],
  loadingPeaks: false,
  error: null,

  windowEndMs: () => get().windowStartMs + get().spanMs,
  coverage: () => get().range?.coverage ?? [],

  activeDay: () => {
    const state = get();
    const centre = state.windowStartMs + state.spanMs / 2;
    return (
      state.days.find((day) => centre >= day.dayStartMs && centre < day.dayEndMs) ?? null
    );
  },

  zoomTo: (spanMs, anchorMs) => {
    const state = get();
    // The limits are policy and live here; holding the anchor is geometry and lives in lib.
    const clamped = Math.min(Math.max(spanMs, MIN_SPAN_MS), MAX_SPAN_MS);
    const zoomed = zoomWindow(
      { startMs: state.windowStartMs, spanMs: state.spanMs },
      clamped,
      anchorMs,
    );

    set({ spanMs: zoomed.spanMs, windowStartMs: zoomed.startMs });
  },

  panBy: (deltaMs) =>
    set((state) => ({
      windowStartMs: state.windowStartMs + deltaMs,
      // Any manual pan means the operator is looking at something specific, so stop dragging the view
      // back to the present under their hands.
      followingLive: false,
    })),

  centreOn: (timestampMs) =>
    set((state) => ({
      windowStartMs: timestampMs - state.spanMs / 2,
      followingLive: false,
    })),

  setFollowingLive: (followingLive) => {
    if (followingLive) {
      const state = get();
      const edge = state.range?.liveEdgeMs ?? Date.now();
      // Leave a tenth of the window as headroom so the playhead is not glued to the right hand edge.
      set({
        followingLive,
        windowStartMs: edge - state.spanMs * 0.9,
      });
      return;
    }
    set({ followingLive });
  },

  /**
   * Frame a whole day's recording on the timeline.
   *
   * The window is fitted to the audio that exists rather than to midnight-to-midnight, because a day
   * holding twenty minutes of recording would otherwise show a sliver of waveform in an ocean of empty
   * timeline and be impossible to click accurately. The fit is padded slightly so the material does not
   * touch the edges, floored at five minutes so a very short recording is still readable, and capped at
   * the day itself so the view never spills into neighbouring days.
   */
  showDay: (dayId) => {
    const state = get();
    const entry = state.days.find((item) => item.day === dayId);
    if (!entry) {
      return;
    }

    const dayLengthMs = entry.dayEndMs - entry.dayStartMs;
    const recordedSpanMs = Math.max(entry.endMs - entry.startMs, 0);
    const spanMs = Math.min(
      Math.max(recordedSpanMs * 1.1, 5 * 60_000, MIN_SPAN_MS),
      dayLengthMs,
      MAX_SPAN_MS,
    );
    const centreMs = (entry.startMs + entry.endMs) / 2;

    set({
      followingLive: false,
      spanMs,
      windowStartMs: centreMs - spanMs / 2,
    });
  },

  refreshDays: async () => {
    try {
      set({ days: await api.recordingDays(), error: null });
    } catch (cause) {
      set({
        error: cause instanceof ApiError ? cause.message : 'could not load the recorded days',
      });
    }
  },

  refreshRange: async () => {
    try {
      const range = await api.timelineRange();
      set((state) => {
        if (!state.followingLive) {
          return { range, error: null };
        }
        const edge = range.liveEdgeMs ?? range.latestMs ?? range.serverTimeMs;
        return { range, error: null, windowStartMs: edge - state.spanMs * 0.9 };
      });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not load the timeline' });
    }
  },

  refreshPeaks: async () => {
    const state = get();
    const fromMs = state.windowStartMs;
    const toMs = state.windowStartMs + state.spanMs;

    set({ loadingPeaks: true });
    try {
      set({ peaks: await api.peaks(fromMs, toMs, PEAK_BUCKETS), error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not load the waveform' });
    } finally {
      set({ loadingPeaks: false });
    }
  },
}));
