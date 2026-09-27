/**
 * The timeline store behaviour that `useTimelineStore.test.ts` and `useTimelineStoreSounds.test.ts` leave
 * out: the derived values (window end, coverage, active day, minimap day), every way the window moves
 * (zoom, pan, scroll, centre, follow live, show a day), and the loaders for the range, the days and the
 * two waveforms, including what each keeps on screen when its request fails.
 *
 * Day arithmetic is local time, so expected day bounds come from `dayBoundsMs` rather than being written
 * out, which keeps the assertions true in whatever timezone the suite runs.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { RecordingDay, TimelineRange } from '@/api/types';
import { dayBoundsMs } from '@/lib/day';

const server = vi.hoisted(() => ({
  calls: [] as unknown[][],
  range: null as unknown,
  rangeFails: null as Error | null,
  days: [] as unknown[],
  daysFails: null as Error | null,
  peaksFails: null as Error | null,
  soundsFail: false,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  return {
    ...actual,
    api: {
      timelineRange: async () => {
        server.calls.push(['timelineRange']);
        if (server.rangeFails) {
          throw server.rangeFails;
        }
        return server.range;
      },
      recordingDays: async () => {
        server.calls.push(['recordingDays']);
        if (server.daysFails) {
          throw server.daysFails;
        }
        return server.days;
      },
      peaks: async (fromMs: number, toMs: number, buckets: number) => {
        server.calls.push(['peaks', fromMs, toMs, buckets]);
        if (server.peaksFails) {
          throw server.peaksFails;
        }
        return { fromMs, toMs, bucketMs: (toMs - fromMs) / buckets, peaks: [buckets] };
      },
      sounds: async (fromMs: number, toMs: number) => {
        server.calls.push(['sounds', fromMs, toMs]);
        if (server.soundsFail) {
          throw new Error('sounds unavailable');
        }
        return { fromMs, toMs, sensitivity: 'medium', sounds: [{ startMs: fromMs, endMs: toMs, seekMs: fromMs, peak: 1 }] };
      },
    },
  };
});

const { ApiError } = await import('@/api/client');
const { DEFAULT_SPAN_MS, useTimelineStore } = await import('../useTimelineStore');

/** Midday local time, far from any daylight saving change. */
const NOON = new Date(2026, 8, 5, 12, 0, 0).getTime();
const TODAY = dayBoundsMs(NOON);

function range(overrides: Partial<TimelineRange> = {}): TimelineRange {
  return {
    earliestMs: NOON - 3_600_000,
    latestMs: NOON - 5_000,
    liveEdgeMs: NOON,
    serverTimeMs: NOON + 1_000,
    coverage: [{ startMs: NOON - 3_600_000, endMs: NOON }],
    ...overrides,
  };
}

function day(startMs: number, endMs: number): RecordingDay {
  const bounds = dayBoundsMs(startMs);
  return {
    day: '2026-09-05',
    startMs,
    endMs,
    dayStartMs: bounds.startMs,
    dayEndMs: bounds.endMs,
    segmentCount: 1,
    bytes: 1,
    recordedMs: endMs - startMs,
  };
}

beforeEach(() => {
  server.calls = [];
  server.range = range();
  server.rangeFails = null;
  server.days = [];
  server.daysFails = null;
  server.peaksFails = null;
  server.soundsFail = false;
  useTimelineStore.setState({
    windowStartMs: NOON - 600_000,
    spanMs: 600_000,
    followingLive: false,
    range: null,
    peaks: null,
    dayPeaks: null,
    sounds: [],
    daySounds: [],
    days: [],
    loadingPeaks: false,
    error: null,
  });
});

describe('derived values', () => {
  it('ends the window a span after it starts', () => {
    expect(useTimelineStore.getState().windowEndMs()).toBe(NOON);
  });

  it('reads coverage from the range, and has none before the range loads', () => {
    expect(useTimelineStore.getState().coverage()).toEqual([]);
    useTimelineStore.setState({ range: range() });
    expect(useTimelineStore.getState().coverage()).toEqual([{ startMs: NOON - 3_600_000, endMs: NOON }]);
  });

  it('frames the minimap on the calendar day holding the middle of the window', () => {
    expect(useTimelineStore.getState().minimapWindow()).toEqual(TODAY);

    // A window straddling midnight belongs to whichever day its middle is in.
    useTimelineStore.setState({ windowStartMs: TODAY.endMs - 100_000, spanMs: 600_000 });
    expect(useTimelineStore.getState().minimapWindow()).toEqual(dayBoundsMs(TODAY.endMs + 1));
  });

  it('finds the recorded day the window sits in, or none', () => {
    const today = day(NOON - 3_600_000, NOON);
    useTimelineStore.setState({ days: [today] });
    expect(useTimelineStore.getState().activeDay()).toEqual(today);

    useTimelineStore.setState({ windowStartMs: TODAY.endMs + 3_600_000 });
    expect(useTimelineStore.getState().activeDay()).toBeNull();
  });

  it('treats the end of a day as belonging to the next one', () => {
    useTimelineStore.setState({ days: [day(NOON - 3_600_000, NOON)], windowStartMs: TODAY.endMs - 300_000 });
    expect(useTimelineStore.getState().activeDay()).toBeNull();
  });
});

describe('zoomTo', () => {
  it('holds the anchor under the same place while changing the span', () => {
    const anchor = NOON - 150_000;
    useTimelineStore.getState().zoomTo(60_000, anchor);

    const state = useTimelineStore.getState();
    expect(state.spanMs).toBe(60_000);
    expect((anchor - state.windowStartMs) / state.spanMs).toBeCloseTo(0.75, 10);
  });

  it('zooms about the centre without an anchor', () => {
    useTimelineStore.getState().zoomTo(60_000);
    const state = useTimelineStore.getState();
    expect(state.windowStartMs + state.spanMs / 2).toBe(NOON - 300_000);
  });

  it('refuses to zoom closer than ten seconds or further than a week', () => {
    useTimelineStore.getState().zoomTo(1);
    expect(useTimelineStore.getState().spanMs).toBe(10_000);

    useTimelineStore.getState().zoomTo(30 * 24 * 3_600_000);
    expect(useTimelineStore.getState().spanMs).toBe(7 * 24 * 3_600_000);
  });

  it('leaves following live as it was', () => {
    useTimelineStore.setState({ followingLive: true });
    useTimelineStore.getState().zoomTo(60_000);
    expect(useTimelineStore.getState().followingLive).toBe(true);
  });
});

describe('moving the window', () => {
  beforeEach(() => {
    useTimelineStore.setState({ followingLive: true });
  });

  it('pans by a distance and stops following live', () => {
    useTimelineStore.getState().panBy(-60_000);
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON - 660_000);
    expect(useTimelineStore.getState().followingLive).toBe(false);
  });

  it('centres on a moment at the same zoom and stops following live', () => {
    useTimelineStore.getState().centreOn(NOON - 3_600_000);
    const state = useTimelineStore.getState();
    expect(state.windowStartMs).toBe(NOON - 3_600_000 - 300_000);
    expect(state.spanMs).toBe(600_000);
    expect(state.followingLive).toBe(false);
  });

  it('scrolls to an absolute start inside the minimap day and stops following live', () => {
    useTimelineStore.getState().scrollTo(NOON - 7_200_000);
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON - 7_200_000);
    expect(useTimelineStore.getState().followingLive).toBe(false);
  });

  it('keeps a scroll inside the day it is dragged within', () => {
    useTimelineStore.getState().scrollTo(TODAY.startMs - 3_600_000);
    expect(useTimelineStore.getState().windowStartMs).toBe(TODAY.startMs);

    useTimelineStore.getState().scrollTo(TODAY.endMs);
    expect(useTimelineStore.getState().windowStartMs).toBe(TODAY.endMs - 600_000);
  });

  it('centres a window wider than the day instead of scrolling it', () => {
    const dayLength = TODAY.endMs - TODAY.startMs;
    useTimelineStore.setState({ windowStartMs: TODAY.startMs, spanMs: dayLength + 2 * 3_600_000 });
    useTimelineStore.getState().scrollTo(TODAY.startMs + 5 * 3_600_000);
    expect(useTimelineStore.getState().windowStartMs).toBe(TODAY.startMs - 3_600_000);
  });
});

describe('setFollowingLive', () => {
  it('puts the live edge a tenth of the window from the right when following starts', () => {
    useTimelineStore.setState({ range: range() });
    useTimelineStore.getState().setFollowingLive(true);

    const state = useTimelineStore.getState();
    expect(state.followingLive).toBe(true);
    expect(state.windowStartMs).toBe(NOON - 540_000);
  });

  it('uses the clock as the edge before the range has loaded', () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOON + 60_000);
    try {
      useTimelineStore.getState().setFollowingLive(true);
      expect(useTimelineStore.getState().windowStartMs).toBe(NOON + 60_000 - 540_000);
    } finally {
      vi.useRealTimers();
    }
  });

  it('leaves the window where it is when following stops', () => {
    useTimelineStore.setState({ followingLive: true });
    useTimelineStore.getState().setFollowingLive(false);
    expect(useTimelineStore.getState().followingLive).toBe(false);
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON - 600_000);
  });
});

describe('showDay', () => {
  it('fits the window to the recording with a little padding, centred on it', () => {
    const recorded = day(NOON - 2 * 3_600_000, NOON);
    useTimelineStore.setState({ days: [recorded], followingLive: true });
    useTimelineStore.getState().showDay('2026-09-05');

    const state = useTimelineStore.getState();
    expect(state.spanMs).toBeCloseTo(2 * 3_600_000 * 1.1, 6);
    expect(state.windowStartMs + state.spanMs / 2).toBeCloseTo(NOON - 3_600_000, 6);
    expect(state.followingLive).toBe(false);
  });

  it('shows at least five minutes of a very short recording', () => {
    useTimelineStore.setState({ days: [day(NOON, NOON + 20_000)] });
    useTimelineStore.getState().showDay('2026-09-05');
    expect(useTimelineStore.getState().spanMs).toBe(5 * 60_000);
  });

  it('never spills past the day itself for a recording that fills it', () => {
    useTimelineStore.setState({ days: [day(TODAY.startMs, TODAY.endMs - 1)] });
    useTimelineStore.getState().showDay('2026-09-05');
    expect(useTimelineStore.getState().spanMs).toBe(TODAY.endMs - TODAY.startMs);
  });

  it('ignores a day it does not know', () => {
    useTimelineStore.getState().showDay('1999-01-01');
    expect(useTimelineStore.getState()).toMatchObject({ windowStartMs: NOON - 600_000, spanMs: 600_000 });
  });
});

describe('refreshDays', () => {
  it('loads the recorded days and clears an earlier error', async () => {
    server.days = [day(NOON - 60_000, NOON)];
    useTimelineStore.setState({ error: 'old problem' });
    await useTimelineStore.getState().refreshDays();

    expect(useTimelineStore.getState().days).toEqual(server.days);
    expect(useTimelineStore.getState().error).toBeNull();
  });

  it('keeps the days it had and says why when loading fails', async () => {
    const known = [day(NOON - 60_000, NOON)];
    useTimelineStore.setState({ days: known });
    server.daysFails = new ApiError('disk not mounted', 'internal', 500);
    await useTimelineStore.getState().refreshDays();

    expect(useTimelineStore.getState().days).toEqual(known);
    expect(useTimelineStore.getState().error).toBe('disk not mounted');
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.daysFails = new Error('boom');
    await useTimelineStore.getState().refreshDays();
    expect(useTimelineStore.getState().error).toBe('could not load the recorded days');
  });
});

describe('refreshRange', () => {
  it('stores the range and leaves the window alone when not following live', async () => {
    await useTimelineStore.getState().refreshRange();
    expect(useTimelineStore.getState().range).toEqual(range());
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON - 600_000);
  });

  it('scrolls to keep the live edge in view when following live', async () => {
    useTimelineStore.setState({ followingLive: true });
    server.range = range({ liveEdgeMs: NOON + 30_000 });
    await useTimelineStore.getState().refreshRange();
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON + 30_000 - 540_000);
  });

  it('falls back to the newest recording, then the server clock, when there is no live edge', async () => {
    useTimelineStore.setState({ followingLive: true });
    server.range = range({ liveEdgeMs: null });
    await useTimelineStore.getState().refreshRange();
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON - 5_000 - 540_000);

    server.range = range({ liveEdgeMs: null, latestMs: null });
    await useTimelineStore.getState().refreshRange();
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON + 1_000 - 540_000);
  });

  it('keeps the range it had and says why when loading fails', async () => {
    useTimelineStore.setState({ range: range() });
    server.rangeFails = new ApiError('sign in first', 'unauthorized', 401);
    await useTimelineStore.getState().refreshRange();

    expect(useTimelineStore.getState().range).toEqual(range());
    expect(useTimelineStore.getState().error).toBe('sign in first');
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.rangeFails = new Error('boom');
    await useTimelineStore.getState().refreshRange();
    expect(useTimelineStore.getState().error).toBe('could not load the timeline');
  });
});

describe('refreshPeaks', () => {
  it('asks for the visible window at 1200 columns', async () => {
    await useTimelineStore.getState().refreshPeaks();
    expect(server.calls).toContainEqual(['peaks', NOON - 600_000, NOON, 1200]);
    expect(server.calls).toContainEqual(['sounds', NOON - 600_000, NOON]);
  });

  it('is loading while the waveform is in flight, and not after', async () => {
    const pending = useTimelineStore.getState().refreshPeaks();
    expect(useTimelineStore.getState().loadingPeaks).toBe(true);
    await pending;
    expect(useTimelineStore.getState().loadingPeaks).toBe(false);
  });

  it('keeps the old waveform and says why when it cannot be loaded, still loading the sounds', async () => {
    useTimelineStore.setState({ peaks: { fromMs: 0, toMs: 1, bucketMs: 1, peaks: [7] } });
    server.peaksFails = new ApiError('disk not mounted', 'internal', 500);
    await useTimelineStore.getState().refreshPeaks();

    const state = useTimelineStore.getState();
    expect(state.peaks?.peaks).toEqual([7]);
    expect(state.error).toBe('disk not mounted');
    expect(state.sounds).toHaveLength(1);
    expect(state.loadingPeaks).toBe(false);
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.peaksFails = new Error('boom');
    await useTimelineStore.getState().refreshPeaks();
    expect(useTimelineStore.getState().error).toBe('could not load the waveform');
  });
});

describe('refreshDayPeaks', () => {
  it('asks for the whole minimap day at 1000 columns', async () => {
    await useTimelineStore.getState().refreshDayPeaks();
    expect(server.calls).toContainEqual(['peaks', TODAY.startMs, TODAY.endMs, 1000]);
    expect(server.calls).toContainEqual(['sounds', TODAY.startMs, TODAY.endMs]);
    expect(useTimelineStore.getState().dayPeaks?.peaks).toEqual([1000]);
  });

  it('keeps the old envelope, still takes the sounds, and raises no error when the envelope fails', async () => {
    const old = { fromMs: 0, toMs: 1, bucketMs: 1, peaks: [3] };
    useTimelineStore.setState({ dayPeaks: old });
    server.peaksFails = new ApiError('disk not mounted', 'internal', 500);
    await useTimelineStore.getState().refreshDayPeaks();

    const state = useTimelineStore.getState();
    expect(state.dayPeaks).toEqual(old);
    expect(state.daySounds).toHaveLength(1);
    expect(state.error).toBeNull();
  });

  it('keeps the old sounds when only the sounds fail', async () => {
    const old = [{ startMs: 1, endMs: 2, seekMs: 0, peak: 5 }];
    useTimelineStore.setState({ daySounds: old });
    server.soundsFail = true;
    await useTimelineStore.getState().refreshDayPeaks();

    expect(useTimelineStore.getState().daySounds).toEqual(old);
    expect(useTimelineStore.getState().dayPeaks?.peaks).toEqual([1000]);
  });
});

describe('bringIntoView edges', () => {
  it('counts both edges of the window as in view', () => {
    useTimelineStore.getState().bringIntoView(NOON - 600_000);
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON - 600_000);
    useTimelineStore.getState().bringIntoView(NOON);
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON - 600_000);
  });

  it('centres on a moment just before the window', () => {
    useTimelineStore.getState().bringIntoView(NOON - 600_001);
    expect(useTimelineStore.getState().windowStartMs).toBe(NOON - 600_001 - 300_000);
  });
});

describe('the standard span', () => {
  it('is fifteen minutes', () => {
    expect(DEFAULT_SPAN_MS).toBe(15 * 60_000);
  });
});
