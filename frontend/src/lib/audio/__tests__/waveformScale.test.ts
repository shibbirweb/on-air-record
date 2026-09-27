/**
 * The Broadcast card's display gain: quiet sound magnified until it fills most of the height, never past
 * the cap, shrunk at once for something louder so nothing clips, and grown back gradually.
 */

import { describe, expect, it } from 'vitest';

import { GROW_RATE, MAX_DISPLAY_GAIN, nextDisplayGain, TARGET_FILL } from '../waveformScale';

describe('nextDisplayGain', () => {
  it('grows towards filling the height, a share of the way each frame', () => {
    // Speech at 5 percent of full scale wants to be drawn 16 times larger.
    const first = nextDisplayGain(1, 0.05);
    expect(first).toBeCloseTo(1 + (MAX_DISPLAY_GAIN - 1) * GROW_RATE);

    let gain = 1;
    for (let frame = 0; frame < 600; frame += 1) {
      gain = nextDisplayGain(gain, 0.1);
    }
    expect(gain * 0.1).toBeCloseTo(TARGET_FILL, 3);
  });

  it('shrinks at once when something louder arrives, so a clap is never clipped', () => {
    expect(nextDisplayGain(16, 0.5)).toBeCloseTo(TARGET_FILL / 0.5);
    expect(nextDisplayGain(16, 1) * 1).toBeCloseTo(TARGET_FILL);
  });

  it('never magnifies past the cap, so room noise stays a ripple', () => {
    let gain = 1;
    for (let frame = 0; frame < 1000; frame += 1) {
      gain = nextDisplayGain(gain, 0.001);
    }
    expect(gain).toBeCloseTo(MAX_DISPLAY_GAIN, 6);
    expect(nextDisplayGain(MAX_DISPLAY_GAIN, 0)).toBe(MAX_DISPLAY_GAIN);
  });

  it('recovers from a gain or a peak that makes no sense', () => {
    for (const current of [Number.NaN, 0, -3, Number.POSITIVE_INFINITY]) {
      expect(Number.isFinite(nextDisplayGain(current, 0.2))).toBe(true);
    }
    expect(nextDisplayGain(2, Number.NaN)).toBeGreaterThanOrEqual(2);
  });
});
