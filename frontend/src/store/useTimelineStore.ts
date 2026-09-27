/**
 * The visible timeline window, its coverage bands and its waveform.
 *
 * The window is the state that matters: everything drawn is a function of it. Peaks are fetched for the
 * window rather than for all of history, which is what keeps an eight hour recording as cheap to draw as
 * an eight minute one.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { CoverageBand, Peaks, RecordingDay, Sound, TimelineRange } from '@/api/types';
import { dayBoundsMs } from '@/lib/day';
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

/** The span the timeline opens at, and the one the reset button returns to. */
export const DEFAULT_SPAN_MS = ZOOM_LEVELS[2];

/**
 * How close to the live edge the playhead must be for a view reset to resume following it.
 *
 * Half a minute is comfortably more than the jitter buffer and the segment rollover, so anyone actually
 * listening to the live feed counts as being at the live edge.
 */
const AT_LIVE_TOLERANCE_MS = 30_000;

/** Zoom limits. Ten seconds shows individual words; a week is past the point of being readable. */
const MIN_SPAN_MS = 10_000;
const MAX_SPAN_MS = 7 * 24 * 3_600_000;

/** Columns requested per fetch. More than a wide screen has pixels is wasted bandwidth. */
const PEAK_BUCKETS = 1200;

/**
 * Columns for the minimap's whole day envelope.
 *
 * A day across a thousand columns is roughly a minute and a half each, which is plenty to show where in
 * the day the audio sits without fetching the detail the main timeline already has.
 */
const MINIMAP_BUCKETS = 1000;

type TimelineState = {
  /** Left edge of the visible window, epoch milliseconds. */
  windowStartMs: number;
  spanMs: number;
  /** True while the window should scroll to keep the live edge at the right. */
  followingLive: boolean;
  range: TimelineRange | null;
  peaks: Peaks | null;
  /** Coarse envelope covering the whole minimap day. */
  dayPeaks: Peaks | null;
  /** The moments something was heard in the visible window, shaded on the timeline. */
  sounds: Sound[];
  /** The same for the minimap's whole day. */
  daySounds: Sound[];
  /** Calendar days that hold recordings, newest first. */
  days: RecordingDay[];
  loadingPeaks: boolean;
  error: string | null;

  windowEndMs: () => number;
  coverage: () => CoverageBand[];
  /** The day entry the visible window sits in, if any. */
  activeDay: () => RecordingDay | null;
  /**
   * The twenty four hours the minimap shows: the calendar day holding the middle of the visible window.
   *
   * Anchored to a calendar day rather than sliding with the viewport, because a minimap that moves as you
   * pan gives you nothing to orient against, which is the whole reason it exists.
   */
  minimapWindow: () => { startMs: number; endMs: number };

  /** Rescale the window, holding `anchorMs` in place. Falls back to the centre when no anchor is given. */
  zoomTo: (spanMs: number, anchorMs?: number | null) => void;
  /** Return to the standard span, framed on whatever is playing. */
  resetView: (anchorMs?: number | null) => void;
  /** Frame one moment at the standard span. Used when jumping to a bookmark. */
  focusOn: (timestampMs: number) => void;
  panBy: (deltaMs: number) => void;
  /** Move the window to start at `startMs`, kept inside the minimap's day. Used by the minimap drag. */
  scrollTo: (startMs: number) => void;
  centreOn: (timestampMs: number) => void;
  setFollowingLive: (following: boolean) => void;
  showDay: (day: string) => void;
  refreshRange: () => Promise<void>;
  refreshPeaks: () => Promise<void>;
  refreshDays: () => Promise<void>;
  refreshDayPeaks: () => Promise<void>;
  /**
   * The sound to jump to from `fromMs`, forward or back, or `null` when there is none that way. The
   * search runs on the server across all of history, not only what is on screen.
   */
  findSound: (fromMs: number, direction: 'forward' | 'backward') => Promise<Sound | null>;
  /**
   * Make sure a moment just jumped to is on screen: centre on it at the current zoom when it is outside
   * the window, and stop following live either way, or the view would scroll away from it.
   */
  bringIntoView: (timestampMs: number) => void;
};

export const useTimelineStore = create<TimelineState>((set, get) => ({
  windowStartMs: Date.now() - DEFAULT_SPAN_MS,
  spanMs: DEFAULT_SPAN_MS,
  followingLive: true,
  range: null,
  peaks: null,
  dayPeaks: null,
  sounds: [],
  daySounds: [],
  days: [],
  loadingPeaks: false,
  error: null,

  windowEndMs: () => get().windowStartMs + get().spanMs,
  coverage: () => get().range?.coverage ?? [],

  minimapWindow: () => {
    const state = get();
    return dayBoundsMs(state.windowStartMs + state.spanMs / 2);
  },

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

  focusOn: (timestampMs) => {
    // Always the standard span, never whatever zoom happened to be in force. Arriving at a marked moment
    // while zoomed out to a day would put it a pixel wide, which is not "taking me there" in any useful
    // sense, and arriving zoomed to a minute would hide its surroundings.
    set({
      spanMs: DEFAULT_SPAN_MS,
      followingLive: false,
      windowStartMs: timestampMs - DEFAULT_SPAN_MS / 2,
    });
  },

  resetView: (anchorMs) => {
    const state = get();
    const liveEdgeMs = state.range?.liveEdgeMs ?? null;

    // Standard means standard for what you are listening to. Someone at the live edge wants the live
    // view back; someone playing yesterday afternoon wants yesterday afternoon at a sane zoom, not to be
    // thrown forward to the present.
    const atLive =
      anchorMs === null ||
      anchorMs === undefined ||
      (liveEdgeMs !== null && liveEdgeMs - anchorMs <= AT_LIVE_TOLERANCE_MS);

    if (atLive) {
      set({ spanMs: DEFAULT_SPAN_MS });
      get().setFollowingLive(true);
      return;
    }

    set({
      spanMs: DEFAULT_SPAN_MS,
      followingLive: false,
      windowStartMs: anchorMs - DEFAULT_SPAN_MS / 2,
    });
  },

  scrollTo: (startMs) => {
    const state = get();
    const { startMs: dayStartMs, endMs: dayEndMs } = state.minimapWindow();
    const dayLengthMs = dayEndMs - dayStartMs;

    // Absolute rather than relative, so a slow drag cannot accumulate rounding error. Clamping keeps the
    // viewport inside the frame it is being dragged within; crossing to another day is the day picker's
    // job. A window wider than the day itself has nowhere to move, so it centres instead.
    const clamped =
      state.spanMs >= dayLengthMs
        ? dayStartMs - (state.spanMs - dayLengthMs) / 2
        : Math.min(Math.max(startMs, dayStartMs), dayEndMs - state.spanMs);

    set({ windowStartMs: clamped, followingLive: false });
  },

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

  refreshDayPeaks: async () => {
    const { startMs, endMs } = get().minimapWindow();

    // Side by side and independently: the sounds are a layer over the envelope, so failing to find them
    // must not cost the envelope, and the other way round.
    const [peaks, sounds] = await Promise.allSettled([
      api.peaks(startMs, endMs, MINIMAP_BUCKETS),
      api.sounds(startMs, endMs),
    ]);
    // The minimap is an orientation aid. Losing either should not raise an error banner over the timeline
    // the listener is actually using, so whatever is stale simply stays on screen.
    if (peaks.status === 'fulfilled') {
      set({ dayPeaks: peaks.value });
    }
    if (sounds.status === 'fulfilled') {
      set({ daySounds: sounds.value.sounds });
    }
  },

  refreshPeaks: async () => {
    const state = get();
    const fromMs = state.windowStartMs;
    const toMs = state.windowStartMs + state.spanMs;

    set({ loadingPeaks: true });
    const sounds = api.sounds(fromMs, toMs).then(
      (window) => set({ sounds: window.sounds }),
      // Sounds are an extra layer; the waveform's own error is the one worth a banner.
      () => undefined,
    );
    try {
      set({ peaks: await api.peaks(fromMs, toMs, PEAK_BUCKETS), error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not load the waveform' });
    } finally {
      set({ loadingPeaks: false });
    }
    await sounds;
  },

  findSound: (fromMs, direction) => api.nextSound(fromMs, direction),

  bringIntoView: (timestampMs) => {
    const state = get();
    const inView =
      timestampMs >= state.windowStartMs && timestampMs <= state.windowStartMs + state.spanMs;
    if (inView) {
      set({ followingLive: false });
      return;
    }
    set({ followingLive: false, windowStartMs: timestampMs - state.spanMs / 2 });
  },
}));
