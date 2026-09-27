// @vitest-environment jsdom

/**
 * The sound layer of the timeline and of the day overview: where each sound is painted, as what, and in
 * which order against the waveform. Drawn onto a recording stand in for the canvas, since jsdom has none,
 * so these check what is painted and where; that it looks right in a real browser is scripts/e2e.mjs.
 *
 * jsdom resolves no stylesheet, so each colour is its fallback: the sound layer paints '#2aa' and the
 * waveform '#f0a'.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { Sound } from '@/api/types';
import { dayBoundsMs } from '@/lib/day';
import { soundBands } from '@/lib/timelineGeometry';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';
import { recordCanvases } from '@/test/canvas';
import type { CanvasRecorder } from '@/test/canvas';

import { TimelineMinimap } from '../TimelineMinimap';
import { TimelineScrubber } from '../TimelineScrubber';

const SOUND_COLOUR = '#2aa';
const WAVE_COLOUR = '#f0a';
const WIDTH = 1000;

/** Noon, so the window sits well inside one calendar day whatever the time zone. */
const WINDOW_START = new Date(2026, 8, 27, 12, 0, 0).getTime();
const SPAN = 600_000;

const sound = (startMs: number, lengthMs: number): Sound => ({
  startMs,
  endMs: startMs + lengthMs,
  seekMs: startMs - 1_000,
  peak: 80,
});

const inView = sound(WINDOW_START + 120_000, 3_000);
const alsoInView = sound(WINDOW_START + 400_000, 8_000);
const offScreen = sound(WINDOW_START + SPAN + 60_000, 2_000);

let canvas: CanvasRecorder;

beforeEach(() => {
  canvas = recordCanvases(WIDTH);
  useBookmarkStore.setState({ bookmarks: [] });
  useTransportStore.setState({ requestedPositionMs: null });
  useTimelineStore.setState({
    windowStartMs: WINDOW_START,
    spanMs: SPAN,
    followingLive: false,
    range: null,
    peaks: {
      fromMs: WINDOW_START,
      toMs: WINDOW_START + SPAN,
      bucketMs: SPAN / 100,
      peaks: Array.from({ length: 100 }, () => 40),
    },
    dayPeaks: null,
    sounds: [inView, alsoInView, offScreen],
    daySounds: [inView, alsoInView],
  });
});

afterEach(() => {
  canvas.restore();
});

const soundPaints = () => canvas.paints.filter((paint) => paint.fillStyle === SOUND_COLOUR);

describe('the timeline', () => {
  it('is labelled, so assistive technology and the browser test can find it', () => {
    render(<TimelineScrubber getPlayheadMs={() => null} />);
    expect(screen.getByRole('img', { name: 'Timeline' })).toBeInTheDocument();
  });

  it('paints each sound on screen as a tint behind the waveform and a solid strip along the bottom', () => {
    render(<TimelineScrubber getPlayheadMs={() => null} />);
    canvas.frame();

    const expected = soundBands([inView, alsoInView, offScreen], {
      startMs: WINDOW_START,
      spanMs: SPAN,
      width: WIDTH,
    });
    expect(expected).toHaveLength(2);

    const tints = soundPaints().filter((paint) => paint.alpha < 1);
    const strips = soundPaints().filter((paint) => paint.alpha === 1);
    expect(tints.map(({ x, width }) => ({ left: x, width }))).toEqual(expected);
    expect(strips.map(({ x, width }) => ({ left: x, width }))).toEqual(expected);

    // The tint covers the waveform's height, below the time ruler; the strip is its bottom four pixels.
    for (const tint of tints) {
      expect(tint).toMatchObject({ y: 22, height: 110, alpha: 0.22 });
    }
    for (const strip of strips) {
      expect(strip).toMatchObject({ y: 128, height: 4 });
    }
  });

  it('paints the sounds before the waveform, so the waveform stays on top', () => {
    render(<TimelineScrubber getPlayheadMs={() => null} />);
    canvas.frame();
    const firstSound = canvas.paints.findIndex((paint) => paint.fillStyle === SOUND_COLOUR);
    const firstWave = canvas.paints.findIndex((paint) => paint.fillStyle === WAVE_COLOUR);
    expect(firstSound).toBeGreaterThanOrEqual(0);
    expect(firstWave).toBeGreaterThan(firstSound);
  });

  it('paints nothing in the sound colour when there are no sounds', () => {
    useTimelineStore.setState({ sounds: [] });
    render(<TimelineScrubber getPlayheadMs={() => null} />);
    canvas.frame();
    expect(soundPaints()).toEqual([]);
  });
});

describe('the day overview', () => {
  it('is labelled, so assistive technology and the browser test can find it', () => {
    render(<TimelineMinimap getPlayheadMs={() => null} />);
    expect(screen.getByRole('img', { name: 'Day overview' })).toBeInTheDocument();
  });

  it("paints the day's sounds as a strip along the bottom of its bar", () => {
    render(<TimelineMinimap getPlayheadMs={() => null} />);
    canvas.frame();

    const day = dayBoundsMs(WINDOW_START + SPAN / 2);
    const expected = soundBands([inView, alsoInView], {
      startMs: day.startMs,
      spanMs: day.endMs - day.startMs,
      width: WIDTH,
    });
    const strips = soundPaints();
    expect(strips.map(({ x, width }) => ({ left: x, width }))).toEqual(expected);
    // The bar starts below the 15 pixel hour labels and fills the rest of the 54 pixel canvas.
    for (const strip of strips) {
      expect(strip).toMatchObject({ y: 50, height: 4 });
    }
  });

  it('paints nothing in the sound colour when the day has no sounds', () => {
    useTimelineStore.setState({ daySounds: [] });
    render(<TimelineMinimap getPlayheadMs={() => null} />);
    canvas.frame();
    expect(soundPaints()).toEqual([]);
  });
});
