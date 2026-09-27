import { describe, expect, it } from 'vitest';

import { noSoundNotice, soundSearchFrom } from '../soundSearch';
import type { SoundSearchContext } from '../soundSearch';

const NOW = 1_757_034_000_000;

/** Nothing playing, nothing cued, the timeline following live, a recording in progress. */
const idle: SoundSearchContext = {
  playheadMs: null,
  cuedMs: null,
  windowStartMs: NOW - 900_000,
  followingLive: true,
  liveEdgeMs: NOW - 2_000,
  nowMs: NOW,
};

describe('soundSearchFrom', () => {
  it('searches from what is being heard, ahead of everything else', () => {
    expect(
      soundSearchFrom({ ...idle, playheadMs: NOW - 60_000, cuedMs: NOW - 5_000, followingLive: false }),
    ).toBe(NOW - 60_000);
  });

  it('searches from the cued moment when nothing plays', () => {
    expect(soundSearchFrom({ ...idle, cuedMs: NOW - 5_000, followingLive: false })).toBe(NOW - 5_000);
  });

  it('searches from the left edge of the timeline when browsing it without playing', () => {
    expect(soundSearchFrom({ ...idle, followingLive: false, windowStartMs: NOW - 86_400_000 })).toBe(
      NOW - 86_400_000,
    );
  });

  it('searches from the live edge when the timeline follows live, ignoring where it happens to start', () => {
    expect(soundSearchFrom(idle)).toBe(NOW - 2_000);
  });

  it('falls back to the clock when following live with nothing being recorded', () => {
    expect(soundSearchFrom({ ...idle, liveEdgeMs: null })).toBe(NOW);
  });

  it('treats a position of zero as a position, not as nothing', () => {
    expect(soundSearchFrom({ ...idle, playheadMs: 0 })).toBe(0);
    expect(soundSearchFrom({ ...idle, cuedMs: 0 })).toBe(0);
    expect(soundSearchFrom({ ...idle, followingLive: false, windowStartMs: 0 })).toBe(0);
  });
});

describe('noSoundNotice', () => {
  it('points forward at going live, and says plainly when there is nothing earlier', () => {
    expect(noSoundNotice('forward')).toMatch(/No later sound/);
    expect(noSoundNotice('forward')).toMatch(/Go live/);
    expect(noSoundNotice('backward')).toBe('No earlier sound in the recordings.');
  });
});
