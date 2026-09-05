import { beforeEach, describe, expect, it } from 'vitest';

import { DEFAULT_SPAN_MS, useTimelineStore, ZOOM_LEVELS } from '../useTimelineStore';

const LIVE_EDGE = 1_757_034_000_000;

/** A range payload with just the fields the view logic reads. */
function rangeAt(liveEdgeMs: number | null) {
  return {
    earliestMs: liveEdgeMs === null ? null : liveEdgeMs - 86_400_000,
    latestMs: liveEdgeMs,
    liveEdgeMs,
    serverTimeMs: liveEdgeMs ?? 0,
    coverage: [],
  };
}

describe('resetView', () => {
  beforeEach(() => {
    useTimelineStore.setState({
      windowStartMs: LIVE_EDGE - 86_400_000,
      spanMs: ZOOM_LEVELS[6],
      followingLive: false,
      range: rangeAt(LIVE_EDGE),
      days: [],
    });
  });

  it('returns to the standard span', () => {
    useTimelineStore.getState().resetView(LIVE_EDGE);
    expect(useTimelineStore.getState().spanMs).toBe(DEFAULT_SPAN_MS);
  });

  it('resumes following live when the playhead is at the live edge', () => {
    useTimelineStore.getState().resetView(LIVE_EDGE - 2_000);

    const state = useTimelineStore.getState();
    expect(state.followingLive).toBe(true);
    // Following puts the live edge near the right hand side rather than dead centre.
    expect(state.windowStartMs).toBeLessThan(LIVE_EDGE);
    expect(state.windowStartMs + state.spanMs).toBeGreaterThan(LIVE_EDGE);
  });

  it('resumes following live when nothing is playing', () => {
    useTimelineStore.getState().resetView(null);
    expect(useTimelineStore.getState().followingLive).toBe(true);
  });

  it('frames the playhead instead of jumping forward when playing history', () => {
    const yesterday = LIVE_EDGE - 20 * 3_600_000;
    useTimelineStore.getState().resetView(yesterday);

    const state = useTimelineStore.getState();
    expect(state.followingLive).toBe(false);
    expect(state.spanMs).toBe(DEFAULT_SPAN_MS);
    // Centred on what is actually being listened to.
    expect(state.windowStartMs + state.spanMs / 2).toBe(yesterday);
  });

  it('treats a playhead just outside the tolerance as history', () => {
    useTimelineStore.getState().resetView(LIVE_EDGE - 60_000);
    expect(useTimelineStore.getState().followingLive).toBe(false);
  });

  it('still resets the span when there is no live edge yet', () => {
    useTimelineStore.setState({ range: rangeAt(null), spanMs: ZOOM_LEVELS[0] });
    useTimelineStore.getState().resetView(LIVE_EDGE);

    expect(useTimelineStore.getState().spanMs).toBe(DEFAULT_SPAN_MS);
  });
});
