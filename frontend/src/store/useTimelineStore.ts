/**
 * The visible timeline window, its coverage bands and its waveform.
 *
 * The window is the state that matters: everything drawn is a function of it. Peaks are fetched for the
 * window rather than for all of history, which is what keeps an eight hour recording as cheap to draw as
 * an eight minute one.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { CoverageBand, Peaks, TimelineRange } from '@/api/types';

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
  loadingPeaks: boolean;
  error: string | null;

  windowEndMs: () => number;
  coverage: () => CoverageBand[];

  setSpan: (spanMs: number) => void;
  panBy: (deltaMs: number) => void;
  centreOn: (timestampMs: number) => void;
  setFollowingLive: (following: boolean) => void;
  refreshRange: () => Promise<void>;
  refreshPeaks: () => Promise<void>;
};

export const useTimelineStore = create<TimelineState>((set, get) => ({
  windowStartMs: Date.now() - DEFAULT_SPAN_MS,
  spanMs: DEFAULT_SPAN_MS,
  followingLive: true,
  range: null,
  peaks: null,
  loadingPeaks: false,
  error: null,

  windowEndMs: () => get().windowStartMs + get().spanMs,
  coverage: () => get().range?.coverage ?? [],

  setSpan: (spanMs) => {
    const state = get();
    // Zoom around the middle of what is on screen, which is what the eye expects, rather than around the
    // left edge, which would slide the content sideways as well as scale it.
    const centre = state.windowStartMs + state.spanMs / 2;
    const clamped = Math.min(Math.max(spanMs, 10_000), 7 * 24 * 3_600_000);
    set({ spanMs: clamped, windowStartMs: centre - clamped / 2 });
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
