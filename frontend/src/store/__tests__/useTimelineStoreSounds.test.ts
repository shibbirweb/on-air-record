import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Sound } from '@/api/types';

const server = vi.hoisted(() => ({
  soundsFail: false,
  calls: [] as string[],
}));

const SOUND: Sound = { startMs: 1_000_000, endMs: 1_003_000, seekMs: 999_000, peak: 80 };

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  return {
    ...actual,
    api: {
      peaks: async (fromMs: number, toMs: number) => ({ fromMs, toMs, bucketMs: 100, peaks: [1, 2, 3] }),
      sounds: async (fromMs: number, toMs: number) => {
        if (server.soundsFail) {
          throw new Error('sounds unavailable');
        }
        return { fromMs, toMs, sensitivity: 'medium', sounds: [SOUND] };
      },
      nextSound: async (fromMs: number, direction: string) => {
        server.calls.push(`${direction} from ${fromMs}`);
        return direction === 'forward' ? SOUND : null;
      },
    },
  };
});

const { useTimelineStore } = await import('../useTimelineStore');

describe('sounds on the timeline', () => {
  beforeEach(() => {
    server.soundsFail = false;
    server.calls = [];
    useTimelineStore.setState({
      windowStartMs: 0,
      spanMs: 600_000,
      followingLive: true,
      peaks: null,
      dayPeaks: null,
      sounds: [],
      daySounds: [],
      error: null,
    });
  });

  it('loads the sounds alongside the waveform', async () => {
    await useTimelineStore.getState().refreshPeaks();
    const state = useTimelineStore.getState();
    expect(state.peaks?.peaks).toEqual([1, 2, 3]);
    expect(state.sounds).toEqual([SOUND]);
  });

  it('keeps the waveform, and says nothing, when the sounds cannot be loaded', async () => {
    server.soundsFail = true;
    await useTimelineStore.getState().refreshPeaks();
    const state = useTimelineStore.getState();
    expect(state.peaks?.peaks).toEqual([1, 2, 3]);
    expect(state.sounds).toEqual([]);
    expect(state.error).toBeNull();
  });

  it('loads the day for the minimap the same way', async () => {
    await useTimelineStore.getState().refreshDayPeaks();
    expect(useTimelineStore.getState().daySounds).toEqual([SOUND]);
  });

  it('asks the server for the next or previous sound from where playback is', async () => {
    const { findSound } = useTimelineStore.getState();
    expect(await findSound(500_000, 'forward')).toEqual(SOUND);
    expect(await findSound(500_000, 'backward')).toBeNull();
    expect(server.calls).toEqual(['forward from 500000', 'backward from 500000']);
  });
});

describe('bringIntoView', () => {
  beforeEach(() => {
    useTimelineStore.setState({ windowStartMs: 0, spanMs: 600_000, followingLive: true });
  });

  it('leaves the window alone when the moment is already on screen, but stops following live', () => {
    useTimelineStore.getState().bringIntoView(300_000);
    const state = useTimelineStore.getState();
    expect(state.windowStartMs).toBe(0);
    expect(state.followingLive).toBe(false);
  });

  it('centres on a moment off screen at the same zoom', () => {
    useTimelineStore.getState().bringIntoView(5_000_000);
    const state = useTimelineStore.getState();
    expect(state.spanMs).toBe(600_000);
    expect(state.windowStartMs).toBe(5_000_000 - 300_000);
  });
});
