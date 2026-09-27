// @vitest-environment jsdom

/**
 * The day overview under the timeline: the hour grid and its labels, the coverage bands, the day's
 * waveform, bookmark ticks, the live edge, the playhead or cue marker, the window box that shows what the
 * timeline above is looking at, and grabbing that box to move the timeline. The sound strip has its own
 * file, soundLayers.test.tsx.
 *
 * Drawn onto a recording stand in for the canvas, since jsdom has none, so these check what is painted and
 * where. jsdom resolves no stylesheet, so every colour is the fallback the component passes to
 * readCssColor: grid '#333', text '#888', coverage '#444', waveform '#f0a', live edge '#f33', marker and
 * window '#fff', bookmarks '#e8a33d'.
 */

import '@/test/dom';

import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { Bookmark } from '@/api/types';
import { dayBoundsMs, dayLabel, localDayId } from '@/lib/day';
import { formatClock } from '@/lib/format';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

import { TimelineMinimap } from '../TimelineMinimap';
import { isVerticalLine, recordDrawing } from './drawing';
import type { DrawingRecorder, PathPaint } from './drawing';

const GRID = '#333';
const TEXT = '#888';
const COVERAGE = '#444';
const WAVE = '#f0a';
const LIVE = '#f33';
const MARKER = '#fff';
const BOOKMARK = '#e8a33d';

/** Forty pixels an hour. */
const WIDTH = 960;
const HEIGHT = 54;
const LABELS = 15;
const BAR = HEIGHT - LABELS;

const NOON = new Date(2026, 8, 16, 12, 0, 0).getTime();
const DAY = dayBoundsMs(NOON);
const DAY_MS = DAY.endMs - DAY.startMs;
const HOUR = 3_600_000;

/** Where a moment of the day falls on the bar. */
const xOf = (timestampMs: number) => ((timestampMs - DAY.startMs) / DAY_MS) * WIDTH;
/** The moment at `hours` past midnight, as the bar measures it. */
const hour = (hours: number) => DAY.startMs + (hours / 24) * DAY_MS;

const pristine = {
  bookmarks: useBookmarkStore.getState(),
  timeline: useTimelineStore.getState(),
  transport: useTransportStore.getState(),
};

let drawing: DrawingRecorder;

function renderMinimap(getPlayheadMs: () => number | null = () => null) {
  const view = render(<TimelineMinimap getPlayheadMs={getPlayheadMs} />);
  const canvas = screen.getByRole('img', { name: 'Day overview' }) as HTMLCanvasElement;
  return { ...view, canvas };
}

function drawFrame() {
  drawing.clear();
  drawing.frame();
}

function placeCanvas(canvas: HTMLCanvasElement) {
  vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue({
    left: 0,
    top: 0,
    right: WIDTH,
    bottom: HEIGHT,
    width: WIDTH,
    height: HEIGHT,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
}

const fills = (colour: string) => drawing.paints.filter((paint) => paint.fillStyle === colour);
const strokes = (colour: string): PathPaint[] =>
  drawing.paths.filter((path) => path.kind === 'stroke' && path.style === colour);

beforeEach(() => {
  drawing = recordDrawing(WIDTH);
  useBookmarkStore.setState(pristine.bookmarks, true);
  useTimelineStore.setState(pristine.timeline, true);
  useTransportStore.setState(pristine.transport, true);
  useBookmarkStore.setState({ bookmarks: [] });
  useTransportStore.setState({ requestedPositionMs: null });
  // An hour from noon: the window box spans 480 to 520.
  useTimelineStore.setState({
    windowStartMs: NOON,
    spanMs: HOUR,
    followingLive: false,
    range: null,
    dayPeaks: null,
    daySounds: [],
  });
});

afterEach(() => {
  drawing.restore();
  vi.restoreAllMocks();
});

describe('the hour grid', () => {
  it('draws a line every hour, full height and stronger every six hours', () => {
    renderMinimap();
    drawFrame();

    const lines = strokes(GRID);
    expect(lines).toHaveLength(25);
    lines.forEach((line, index) => {
      const x = Math.round((index / 24) * WIDTH) + 0.5;
      const major = index % 6 === 0;
      expect(isVerticalLine(line, x, major ? LABELS : LABELS + BAR * 0.65, HEIGHT)).toBe(true);
      expect(line.alpha).toBe(major ? 0.7 : 0.3);
    });
  });

  it('labels midnight, six, noon and eighteen hundred, and not the closing midnight', () => {
    renderMinimap();
    drawFrame();
    expect(drawing.texts).toEqual([
      { text: '00:00', x: 3.5, y: LABELS / 2, fillStyle: TEXT },
      { text: '06:00', x: 243.5, y: LABELS / 2, fillStyle: TEXT },
      { text: '12:00', x: 483.5, y: LABELS / 2, fillStyle: TEXT },
      { text: '18:00', x: 723.5, y: LABELS / 2, fillStyle: TEXT },
    ]);
  });
});

describe('the coverage bands', () => {
  it('shade the recorded stretches of the day across the bar', () => {
    useTimelineStore.setState({
      range: {
        earliestMs: hour(6),
        latestMs: hour(9),
        liveEdgeMs: null,
        serverTimeMs: hour(9),
        coverage: [
          { startMs: hour(6), endMs: hour(9) },
          // Yesterday, off the bar.
          { startMs: DAY.startMs - 5 * HOUR, endMs: DAY.startMs - 4 * HOUR },
        ],
      },
    });
    renderMinimap();
    drawFrame();

    const bands = fills(COVERAGE);
    expect(bands).toHaveLength(1);
    expect(bands[0]).toMatchObject({ y: LABELS, height: BAR, alpha: 0.4 });
    expect(bands[0].x).toBeCloseTo(xOf(hour(6)));
    expect(bands[0].width).toBeCloseTo(xOf(hour(9)) - xOf(hour(6)));
  });
});

describe("the day's waveform", () => {
  it('draws the envelope about the middle of the bar, at most four fifths of its height', () => {
    const peaks = Array.from({ length: 24 }, () => 0);
    peaks[6] = 255;
    useTimelineStore.setState({
      dayPeaks: { fromMs: DAY.startMs, toMs: DAY.endMs, bucketMs: HOUR, peaks },
    });
    renderMinimap();
    drawFrame();

    const bars = fills(WAVE);
    expect(bars).toHaveLength(40);
    for (const bar of bars) {
      expect(bar.x).toBeGreaterThanOrEqual(240);
      expect(bar.x).toBeLessThan(280);
      expect(bar.height).toBeCloseTo(BAR * 0.8);
      expect(bar.y + bar.height / 2).toBeCloseTo(LABELS + BAR / 2);
    }
  });

  it('draws nothing before the envelope has loaded', () => {
    renderMinimap();
    drawFrame();
    expect(fills(WAVE)).toEqual([]);
  });
});

describe('the bookmark ticks', () => {
  const mark = (id: number, timestampMs: number): Bookmark => ({
    id,
    timestampMs,
    label: `Mark ${id}`,
    note: null,
    createdAtMs: timestampMs,
  });

  it('are short ticks at the top of the bar, with no label', () => {
    useBookmarkStore.setState({ bookmarks: [mark(1, hour(3))] });
    renderMinimap();
    drawFrame();

    const ticks = fills(BOOKMARK);
    expect(ticks).toHaveLength(1);
    expect(ticks[0]).toMatchObject({ y: LABELS, width: 2, height: 5 });
    expect(ticks[0].x).toBeCloseTo(119);
    expect(drawing.texts.map((text) => text.text)).not.toContain('Mark 1');
  });

  it('leave out bookmarks from other days', () => {
    useBookmarkStore.setState({ bookmarks: [mark(1, DAY.startMs - HOUR), mark(2, DAY.endMs + HOUR)] });
    renderMinimap();
    drawFrame();
    expect(fills(BOOKMARK)).toEqual([]);
  });
});

describe('the live edge', () => {
  it('is a red line at the present moment when it falls in the day', () => {
    useTimelineStore.setState({
      range: { earliestMs: hour(1), latestMs: hour(15), liveEdgeMs: hour(15), serverTimeMs: hour(15), coverage: [] },
    });
    renderMinimap();
    drawFrame();

    const lines = strokes(LIVE);
    expect(lines).toHaveLength(1);
    expect(lines[0].points[0].x).toBeCloseTo(600);
    expect(lines[0]).toMatchObject({ lineWidth: 1.5 });
    expect(lines[0].points.map((point) => point.y)).toEqual([LABELS, HEIGHT]);
  });

  it('is not drawn on a day before today', () => {
    useTimelineStore.setState({
      range: {
        earliestMs: hour(1),
        latestMs: DAY.endMs + 5 * HOUR,
        liveEdgeMs: DAY.endMs + 5 * HOUR,
        serverTimeMs: DAY.endMs + 5 * HOUR,
        coverage: [],
      },
    });
    renderMinimap();
    drawFrame();
    expect(strokes(LIVE)).toEqual([]);
  });
});

describe('the playhead marker', () => {
  const markerLines = () => strokes(MARKER);

  it('is drawn where audio is coming out', () => {
    renderMinimap(() => hour(10));
    drawFrame();
    expect(markerLines()).toHaveLength(1);
    expect(markerLines()[0].points[0].x).toBeCloseTo(400);
    expect(markerLines()[0]).toMatchObject({ lineWidth: 1.5, alpha: 0.9 });
  });

  it('falls back to the cued moment while nothing plays', () => {
    useTransportStore.setState({ requestedPositionMs: hour(20) });
    renderMinimap(() => null);
    drawFrame();
    expect(markerLines()).toHaveLength(1);
    expect(markerLines()[0].points[0].x).toBeCloseTo(800);
  });

  it('is not drawn when neither playing nor cued', () => {
    renderMinimap(() => null);
    drawFrame();
    expect(markerLines()).toEqual([]);
  });
});

describe('the window box', () => {
  const box = () => fills(MARKER).filter((paint) => paint.alpha < 1);
  const handles = () => fills(MARKER).filter((paint) => paint.alpha === 1);

  it('highlights the slice of the day the timeline shows, outlined, with a grab handle at each side', () => {
    renderMinimap();
    drawFrame();

    expect(box()).toHaveLength(1);
    expect(box()[0]).toMatchObject({ y: LABELS, height: BAR, alpha: 0.16 });
    expect(box()[0].x).toBeCloseTo(480);
    expect(box()[0].width).toBeCloseTo(40);

    expect(drawing.rectStrokes).toHaveLength(1);
    expect(drawing.rectStrokes[0]).toMatchObject({ y: LABELS + 0.5, height: BAR - 1, alpha: 0.9 });
    expect(drawing.rectStrokes[0].x).toBeCloseTo(480.5);
    expect(drawing.rectStrokes[0].width).toBeCloseTo(39);

    expect(handles().map(({ x, y, width, height }) => ({ x: Math.round(x), y, width, height }))).toEqual([
      { x: 480, y: LABELS + 2, width: 2, height: BAR - 4 },
      { x: 518, y: LABELS + 2, width: 2, height: BAR - 4 },
    ]);
  });

  it('is kept wide enough to grab when the timeline shows only a minute', () => {
    useTimelineStore.setState({ spanMs: 60_000 });
    renderMinimap();
    drawFrame();
    expect(box()[0].width).toBe(10);
  });

  it('stays on the bar when the window reaches the very end of the day', () => {
    useTimelineStore.setState({ windowStartMs: DAY.endMs - 70_000, spanMs: 60_000 });
    renderMinimap();
    drawFrame();
    expect(box()[0].x).toBe(WIDTH - 10);
    expect(box()[0].width).toBe(10);
  });

  it('is drawn last, over everything else', () => {
    useTimelineStore.setState({
      range: {
        earliestMs: hour(1),
        latestMs: hour(12),
        liveEdgeMs: hour(12),
        serverTimeMs: hour(12),
        coverage: [{ startMs: hour(1), endMs: hour(12) }],
      },
    });
    useBookmarkStore.setState({
      bookmarks: [{ id: 1, timestampMs: hour(12.5), label: 'Mark', note: null, createdAtMs: hour(12.5) }],
    });
    renderMinimap();
    drawFrame();

    // The box and its two handles are the final three rectangles of the frame.
    const lastThree = drawing.paints.slice(-3);
    expect(lastThree.map((paint) => [paint.fillStyle, paint.alpha])).toEqual([
      [MARKER, 0.16],
      [MARKER, 1],
      [MARKER, 1],
    ]);
  });
});

describe('grabbing the window box', () => {
  let scrollTo: Mock<(startMs: number) => void>;

  beforeEach(() => {
    scrollTo = vi.fn<(startMs: number) => void>();
    useTimelineStore.setState({ scrollTo });
  });

  it('holds the window where it is under the pointer, then carries it along', () => {
    const { canvas } = renderMinimap();
    placeCanvas(canvas);

    // 12:30, inside the noon to one o'clock box.
    fireEvent.pointerDown(canvas, { clientX: 500, pointerId: 1 });
    expect(scrollTo).toHaveBeenLastCalledWith(NOON);

    // An hour to the right.
    fireEvent.pointerMove(canvas, { clientX: 540, pointerId: 1 });
    expect(scrollTo).toHaveBeenLastCalledWith(hour(13));
  });

  it('jumps the window to centre on a press outside it, then drags from the middle', () => {
    const { canvas } = renderMinimap();
    placeCanvas(canvas);

    fireEvent.pointerDown(canvas, { clientX: 240, pointerId: 1 });
    expect(scrollTo).toHaveBeenLastCalledWith(hour(5.5));

    fireEvent.pointerMove(canvas, { clientX: 280, pointerId: 1 });
    expect(scrollTo).toHaveBeenLastCalledWith(hour(6.5));
  });

  it('lets go when the pointer is released', () => {
    const { canvas } = renderMinimap();
    placeCanvas(canvas);

    fireEvent.pointerDown(canvas, { clientX: 500, pointerId: 1 });
    fireEvent.pointerUp(canvas, { clientX: 500, pointerId: 1 });
    scrollTo.mockClear();
    fireEvent.pointerMove(canvas, { clientX: 700, pointerId: 1 });

    expect(scrollTo).not.toHaveBeenCalled();
  });

  it('lets go when the browser cancels the pointer', () => {
    const { canvas } = renderMinimap();
    placeCanvas(canvas);

    fireEvent.pointerDown(canvas, { clientX: 500, pointerId: 1 });
    fireEvent.pointerCancel(canvas, { pointerId: 1 });
    scrollTo.mockClear();
    fireEvent.pointerMove(canvas, { clientX: 700, pointerId: 1 });

    expect(scrollTo).not.toHaveBeenCalled();
  });

  it('does nothing when the pointer only passes over', () => {
    const { canvas } = renderMinimap();
    placeCanvas(canvas);
    fireEvent.pointerMove(canvas, { clientX: 700, pointerId: 1 });
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it('ignores a second pointer while the first holds the box', () => {
    const { canvas } = renderMinimap();
    placeCanvas(canvas);

    fireEvent.pointerDown(canvas, { clientX: 500, pointerId: 1 });
    scrollTo.mockClear();
    fireEvent.pointerMove(canvas, { clientX: 700, pointerId: 2 });

    expect(scrollTo).not.toHaveBeenCalled();
  });

  it('shows a grabbing hand while held and an open one otherwise', () => {
    const { canvas } = renderMinimap();
    placeCanvas(canvas);
    expect(canvas).toHaveClass('cursor-grab');

    fireEvent.pointerDown(canvas, { clientX: 500, pointerId: 1 });
    expect(canvas).toHaveClass('cursor-grabbing');

    fireEvent.pointerUp(canvas, { clientX: 500, pointerId: 1 });
    expect(canvas).toHaveClass('cursor-grab');
    expect(canvas).not.toHaveClass('cursor-grabbing');
  });

  it('moves the real timeline, kept inside the day', () => {
    useTimelineStore.setState({ scrollTo: pristine.timeline.scrollTo });
    const { canvas } = renderMinimap();
    placeCanvas(canvas);

    fireEvent.pointerDown(canvas, { clientX: 500, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 2_000, pointerId: 1 });

    expect(useTimelineStore.getState().windowStartMs).toBe(DAY.endMs - HOUR);
    expect(useTimelineStore.getState().followingLive).toBe(false);
  });
});

describe('the readout under the overview', () => {
  it('names the day it frames and its first and last second', () => {
    renderMinimap();
    expect(screen.getByText(formatClock(DAY.startMs))).toBeInTheDocument();
    expect(screen.getByText(formatClock(DAY.endMs - 1))).toBeInTheDocument();
    expect(
      screen.getByText(
        `${dayLabel(localDayId(NOON), null, null)} overview, drag the window to move the timeline`,
      ),
    ).toBeInTheDocument();
  });

  it('frames the calendar day holding the middle of the window, not a rolling day', () => {
    const tomorrowNoon = DAY.endMs + 12 * HOUR;
    useTimelineStore.setState({ windowStartMs: tomorrowNoon });
    renderMinimap();
    expect(
      screen.getByText(
        `${dayLabel(localDayId(tomorrowNoon), null, null)} overview, drag the window to move the timeline`,
      ),
    ).toBeInTheDocument();
  });
});
