// @vitest-environment jsdom

/**
 * The timeline canvas, layer by layer and gesture by gesture: the coverage bands that say what was
 * recorded, the waveform, the time grid and its labels, bookmark flags, the live edge, the playhead and the
 * cue marker, the hover cursor and its readout, and click to seek, drag to pan and scroll to zoom. The
 * sound layer has its own file, soundLayers.test.tsx.
 *
 * Drawn onto a recording stand in for the canvas, since jsdom has none, so these check what is painted and
 * where; that it looks right is for scripts/e2e.mjs. jsdom resolves no stylesheet, so every colour is the
 * fallback the component passes to readCssColor: coverage '#444', waveform '#f0a', grid '#333', text
 * '#888', live edge '#f33', playhead '#fff', card surface '#111' and bookmarks '#e8a33d'.
 */

import '@/test/dom';

import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { Bookmark } from '@/api/types';
import { formatClock } from '@/lib/format';
import { chooseTickStepMs, tickTimestamps } from '@/lib/timelineGeometry';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

import { TimelineScrubber } from '../TimelineScrubber';
import { isVerticalLine, recordDrawing } from './drawing';
import type { DrawingRecorder, PathPaint } from './drawing';

const COVERAGE = '#444';
const WAVE = '#f0a';
const GRID = '#333';
const TEXT = '#888';
const LIVE = '#f33';
const PLAYHEAD = '#fff';
const SURFACE = '#111';
const BOOKMARK = '#e8a33d';

const WIDTH = 1000;
const HEIGHT = 132;
const RULER = 22;

/** Noon on an ordinary day, so the window sits well inside one calendar day whatever the time zone. */
const START = new Date(2026, 8, 16, 12, 0, 0).getTime();
/** Ten minutes across a thousand pixels: one pixel is 600 ms. */
const SPAN = 600_000;
const MS_PER_PX = SPAN / WIDTH;

/** The moment `x` pixels into the window. */
const at = (x: number) => START + x * MS_PER_PX;

const bookmark = (id: number, x: number, label: string): Bookmark => ({
  id,
  timestampMs: at(x),
  label,
  note: null,
  createdAtMs: START,
});

/** The stores as they start, actions included, so a spy put in by one test never leaks into the next. */
const pristine = {
  bookmarks: useBookmarkStore.getState(),
  timeline: useTimelineStore.getState(),
  transport: useTransportStore.getState(),
};

let drawing: DrawingRecorder;
let seek: Mock<(timestampMs: number) => void>;

function renderScrubber(getPlayheadMs: () => number | null = () => null) {
  const view = render(<TimelineScrubber getPlayheadMs={getPlayheadMs} />);
  const canvas = screen.getByRole('img', { name: 'Timeline' }) as HTMLCanvasElement;
  return { ...view, canvas };
}

/** Draw one fresh frame, forgetting whatever earlier frames painted. */
function drawFrame() {
  drawing.clear();
  drawing.frame();
}

/** jsdom lays nothing out, so the canvas is given a position and size for the pointer maths. */
function placeCanvas(canvas: HTMLCanvasElement, left = 100) {
  vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue({
    left,
    top: 0,
    right: left + WIDTH,
    bottom: HEIGHT,
    width: WIDTH,
    height: HEIGHT,
    x: left,
    y: 0,
    toJSON: () => ({}),
  });
}

const fills = (colour: string) => drawing.paints.filter((paint) => paint.fillStyle === colour);
const strokes = (colour: string): PathPaint[] =>
  drawing.paths.filter((path) => path.kind === 'stroke' && path.style === colour);
const pathFills = (colour: string): PathPaint[] =>
  drawing.paths.filter((path) => path.kind === 'fill' && path.style === colour);

beforeEach(() => {
  drawing = recordDrawing(WIDTH);
  seek = vi.fn<(timestampMs: number) => void>();
  useBookmarkStore.setState(pristine.bookmarks, true);
  useTimelineStore.setState(pristine.timeline, true);
  useTransportStore.setState(pristine.transport, true);
  useBookmarkStore.setState({ bookmarks: [] });
  useTransportStore.setState({ requestedPositionMs: null, seek });
  useTimelineStore.setState({
    windowStartMs: START,
    spanMs: SPAN,
    followingLive: false,
    range: null,
    peaks: null,
    sounds: [],
  });
});

afterEach(() => {
  drawing.restore();
  vi.restoreAllMocks();
});

describe('the coverage bands', () => {
  it('shade each recorded stretch across the waveform area, below the time ruler', () => {
    useTimelineStore.setState({
      range: {
        earliestMs: START,
        latestMs: at(300),
        liveEdgeMs: null,
        serverTimeMs: at(300),
        coverage: [{ startMs: at(100), endMs: at(300) }],
      },
    });
    renderScrubber();
    drawFrame();

    expect(fills(COVERAGE)).toEqual([
      { x: 100, y: RULER, width: 200, height: HEIGHT - RULER, fillStyle: COVERAGE, alpha: 0.35 },
    ]);
  });

  it('skip a stretch that lies entirely outside the window', () => {
    useTimelineStore.setState({
      range: {
        earliestMs: START - 7_200_000,
        latestMs: at(500),
        liveEdgeMs: null,
        serverTimeMs: at(500),
        coverage: [
          { startMs: START - 7_200_000, endMs: START - 3_600_000 },
          { startMs: at(400), endMs: at(500) },
          { startMs: at(WIDTH) + 60_000, endMs: at(WIDTH) + 120_000 },
        ],
      },
    });
    renderScrubber();
    drawFrame();

    expect(fills(COVERAGE).map(({ x, width }) => ({ x, width }))).toEqual([{ x: 400, width: 100 }]);
  });

  it('keep a stretch shorter than a pixel visible as a one pixel sliver', () => {
    useTimelineStore.setState({
      range: {
        earliestMs: START,
        latestMs: at(600),
        liveEdgeMs: null,
        serverTimeMs: at(600),
        coverage: [{ startMs: at(600), endMs: at(600) + 100 }],
      },
    });
    renderScrubber();
    drawFrame();

    expect(fills(COVERAGE)).toHaveLength(1);
    expect(fills(COVERAGE)[0]).toMatchObject({ x: 600, width: 1 });
  });

  it('paint nothing when nothing has been recorded yet', () => {
    renderScrubber();
    drawFrame();
    expect(fills(COVERAGE)).toEqual([]);
  });
});

describe('the waveform', () => {
  beforeEach(() => {
    // Ten buckets over the first half of the window: fifty pixels each.
    useTimelineStore.setState({
      peaks: {
        fromMs: START,
        toMs: at(500),
        bucketMs: 30_000,
        peaks: [255, 0, 51, 1, 0, 0, 0, 0, 0, 0],
      },
    });
  });

  it('draws one bar per pixel, symmetric about the centre line, as tall as the level', () => {
    renderScrubber();
    drawFrame();
    const bars = fills(WAVE);

    // A full scale bucket fills the whole height under the ruler.
    const loud = bars.filter((bar) => bar.x < 50);
    expect(loud).toHaveLength(50);
    for (const bar of loud) {
      expect(bar).toMatchObject({ y: RULER, width: 1, height: HEIGHT - RULER });
    }

    // A fifth of full scale is a fifth of the height, still centred.
    const quieter = bars.filter((bar) => bar.x >= 100 && bar.x < 150);
    expect(quieter).toHaveLength(50);
    for (const bar of quieter) {
      expect(bar.height).toBeCloseTo(22);
      expect(bar.y + bar.height / 2).toBeCloseTo(RULER + (HEIGHT - RULER) / 2);
    }
  });

  it('leaves silence undrawn but keeps the faintest level visible as a one pixel bar', () => {
    renderScrubber();
    drawFrame();
    const bars = fills(WAVE);

    expect(bars.filter((bar) => bar.x >= 50 && bar.x < 100)).toEqual([]);
    const faint = bars.filter((bar) => bar.x >= 150 && bar.x < 200);
    expect(faint).toHaveLength(50);
    for (const bar of faint) {
      expect(bar.height).toBe(1);
    }
  });

  it('draws nothing where the envelope holds no data', () => {
    renderScrubber();
    drawFrame();
    expect(fills(WAVE).filter((bar) => bar.x >= 500)).toEqual([]);
  });

  it('draws nothing before the envelope has loaded', () => {
    useTimelineStore.setState({ peaks: null });
    renderScrubber();
    drawFrame();
    expect(fills(WAVE)).toEqual([]);
  });
});

describe('the time grid', () => {
  it('draws a faint full height line and a clock label at every tick', () => {
    renderScrubber();
    drawFrame();

    const view = { startMs: START, spanMs: SPAN, width: WIDTH };
    const ticks = tickTimestamps(view, chooseTickStepMs(view));
    expect(ticks.length).toBeGreaterThanOrEqual(2);

    const lines = strokes(GRID);
    expect(lines).toHaveLength(ticks.length);
    ticks.forEach((tick, index) => {
      const x = Math.round((tick - START) / MS_PER_PX) + 0.5;
      expect(isVerticalLine(lines[index], x, RULER, HEIGHT)).toBe(true);
      expect(lines[index].alpha).toBe(0.5);
    });

    expect(drawing.texts.filter((text) => text.fillStyle === TEXT)).toEqual(
      ticks.map((tick) => ({
        text: formatClock(tick),
        x: Math.round((tick - START) / MS_PER_PX) + 0.5 + 4,
        y: RULER / 2,
        fillStyle: TEXT,
      })),
    );
  });

  it('labels a ten minute window every five minutes', () => {
    renderScrubber();
    drawFrame();
    expect(drawing.texts.map((text) => text.text)).toEqual([
      formatClock(START),
      formatClock(START + 300_000),
      formatClock(START + 600_000),
    ]);
  });
});

describe('the live edge', () => {
  const withLiveEdge = (liveEdgeMs: number) =>
    useTimelineStore.setState({
      range: { earliestMs: START, latestMs: liveEdgeMs, liveEdgeMs, serverTimeMs: liveEdgeMs, coverage: [] },
    });

  it('is a red line at the present moment', () => {
    withLiveEdge(at(750));
    renderScrubber();
    drawFrame();

    const lines = strokes(LIVE);
    expect(lines).toHaveLength(1);
    expect(isVerticalLine(lines[0], 750, RULER, HEIGHT)).toBe(true);
    expect(lines[0].lineWidth).toBe(2);
  });

  it('is not drawn while the window looks at the past', () => {
    withLiveEdge(at(WIDTH) + 3_600_000);
    renderScrubber();
    drawFrame();
    expect(strokes(LIVE)).toEqual([]);
  });
});

describe('the bookmark flags', () => {
  it('stand as a pole with a pennant and the label beside it', () => {
    useBookmarkStore.setState({ bookmarks: [bookmark(1, 200, 'Doorbell')] });
    renderScrubber();
    drawFrame();

    const poles = strokes(BOOKMARK);
    expect(poles).toHaveLength(1);
    expect(isVerticalLine(poles[0], 200, RULER, HEIGHT)).toBe(true);
    expect(poles[0]).toMatchObject({ lineWidth: 1.5, alpha: 0.85 });

    expect(pathFills(BOOKMARK).map((pennant) => pennant.points)).toEqual([
      [
        { x: 200, y: RULER },
        { x: 209, y: RULER + 4 },
        { x: 200, y: RULER + 8 },
      ],
    ]);

    expect(drawing.texts).toContainEqual({ text: 'Doorbell', x: 212, y: RULER + 1, fillStyle: TEXT });
  });

  it('shorten a label that would run into the next flag', () => {
    useBookmarkStore.setState({
      bookmarks: [bookmark(1, 200, 'Front door opened'), bookmark(2, 300, 'Kettle')],
    });
    renderScrubber();
    drawFrame();

    // 100 pixels to the next flag, less the 14 pixel gap, leaves room for eleven characters and "...".
    const labels = drawing.texts.filter((text) => text.y === RULER + 1).map((text) => text.text);
    expect(labels).toEqual(['Front door ...', 'Kettle']);
  });

  it('degrade to a bare flag when the next one is too close for any text', () => {
    useBookmarkStore.setState({
      bookmarks: [bookmark(1, 200, 'Doorbell'), bookmark(2, 230, 'Kettle')],
    });
    renderScrubber();
    drawFrame();

    expect(strokes(BOOKMARK)).toHaveLength(2);
    expect(pathFills(BOOKMARK)).toHaveLength(2);
    const labels = drawing.texts.filter((text) => text.y === RULER + 1).map((text) => text.text);
    expect(labels).toEqual(['Kettle']);
  });

  it('are not drawn for bookmarks well outside the window', () => {
    useBookmarkStore.setState({
      bookmarks: [bookmark(1, -500, 'Earlier'), bookmark(2, WIDTH + 500, 'Later')],
    });
    renderScrubber();
    drawFrame();

    expect(strokes(BOOKMARK)).toEqual([]);
    expect(drawing.texts.map((text) => text.text)).not.toContain('Earlier');
    expect(drawing.texts.map((text) => text.text)).not.toContain('Later');
  });

  it('are drawn before the playhead, so a flag never hides it', () => {
    useBookmarkStore.setState({ bookmarks: [bookmark(1, 500, 'Doorbell')] });
    renderScrubber(() => at(500));
    drawFrame();

    const lastFlag = drawing.paths.findLastIndex((path) => path.style === BOOKMARK);
    const playhead = drawing.paths.findIndex((path) => path.style === PLAYHEAD);
    expect(lastFlag).toBeGreaterThanOrEqual(0);
    expect(playhead).toBeGreaterThan(lastFlag);
  });
});

describe('the playhead and the cue marker', () => {
  it('draws a solid line with a filled head where audio is coming out', () => {
    renderScrubber(() => at(500));
    drawFrame();

    const lines = strokes(PLAYHEAD).filter((path) => path.points.length === 2);
    expect(lines).toHaveLength(1);
    expect(isVerticalLine(lines[0], 500, RULER, HEIGHT)).toBe(true);
    expect(lines[0]).toMatchObject({ lineWidth: 2, alpha: 1 });

    expect(pathFills(PLAYHEAD).map((head) => head.points)).toEqual([
      [
        { x: 495, y: RULER },
        { x: 505, y: RULER },
        { x: 500, y: RULER + 7 },
      ],
    ]);
  });

  it('falls back to a hollow head at the cued moment while nothing plays', () => {
    useTransportStore.setState({ requestedPositionMs: at(300) });
    renderScrubber(() => null);
    drawFrame();

    const lines = strokes(PLAYHEAD).filter((path) => path.points.length === 2);
    expect(lines).toHaveLength(1);
    expect(isVerticalLine(lines[0], 300, RULER, HEIGHT)).toBe(true);
    expect(lines[0].alpha).toBe(0.85);

    // Hollow: filled with the card colour and outlined, rather than filled solid.
    expect(pathFills(PLAYHEAD)).toEqual([]);
    expect(pathFills(SURFACE)).toHaveLength(1);
    const outline = strokes(PLAYHEAD).filter((path) => path.points.length === 3);
    expect(outline).toHaveLength(1);
    expect(outline[0].lineWidth).toBe(1.5);
  });

  it('follows the audio clock over the cued moment once audio is playing', () => {
    useTransportStore.setState({ requestedPositionMs: at(300) });
    renderScrubber(() => at(700));
    drawFrame();

    const lines = strokes(PLAYHEAD).filter((path) => path.points.length === 2);
    expect(lines).toHaveLength(1);
    expect(lines[0].points[0].x).toBe(700);
  });

  it('moves with the audio clock from frame to frame without a render', () => {
    let playheadMs = at(100);
    renderScrubber(() => playheadMs);
    drawFrame();
    expect(strokes(PLAYHEAD)[0].points[0].x).toBe(100);

    playheadMs = at(101);
    drawFrame();
    expect(strokes(PLAYHEAD)[0].points[0].x).toBe(101);
  });

  it('is not drawn when neither playing nor cued', () => {
    renderScrubber(() => null);
    drawFrame();
    expect(strokes(PLAYHEAD)).toEqual([]);
    expect(pathFills(SURFACE)).toEqual([]);
  });

  it('is not drawn when the moment is off screen', () => {
    renderScrubber(() => at(WIDTH) + 60_000);
    drawFrame();
    expect(strokes(PLAYHEAD)).toEqual([]);
  });
});

describe('the hover cursor', () => {
  it('draws a dashed line and reads out the moment under the pointer', () => {
    const { canvas } = renderScrubber();
    placeCanvas(canvas);
    expect(screen.getByText('click to seek, drag to pan, scroll to zoom')).toBeInTheDocument();

    fireEvent.pointerMove(canvas, { clientX: 350, pointerId: 1 });
    drawFrame();

    const dashed = drawing.paths.filter((path) => path.dashed);
    expect(dashed).toHaveLength(1);
    expect(dashed[0]).toMatchObject({ style: TEXT, alpha: 0.6 });
    expect(isVerticalLine(dashed[0], 250, RULER, HEIGHT)).toBe(true);
    expect(screen.getByText(formatClock(at(250)))).toBeInTheDocument();
    expect(screen.queryByText('click to seek, drag to pan, scroll to zoom')).not.toBeInTheDocument();
  });

  it('goes away when the pointer leaves, and the hint comes back', () => {
    const { canvas } = renderScrubber();
    placeCanvas(canvas);
    fireEvent.pointerMove(canvas, { clientX: 350, pointerId: 1 });
    fireEvent.pointerLeave(canvas, { pointerId: 1 });
    drawFrame();

    expect(drawing.paths.filter((path) => path.dashed)).toEqual([]);
    expect(screen.getByText('click to seek, drag to pan, scroll to zoom')).toBeInTheDocument();
  });
});

describe('the readout under the timeline', () => {
  it('shows the times at the two edges of the window', () => {
    renderScrubber();
    expect(screen.getByText(formatClock(START))).toBeInTheDocument();
    expect(screen.getByText(formatClock(START + SPAN))).toBeInTheDocument();
  });
});

describe('click to seek', () => {
  it('seeks to the moment under the pointer, measured from the left of the canvas', () => {
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 100);

    fireEvent.pointerDown(canvas, { clientX: 350, pointerId: 1 });
    fireEvent.pointerUp(canvas, { clientX: 350, pointerId: 1 });

    expect(seek).toHaveBeenCalledTimes(1);
    expect(seek).toHaveBeenCalledWith(at(250));
  });

  it('still counts as a click when the hand wobbles a few pixels', () => {
    const panBy = vi.fn<(deltaMs: number) => void>();
    useTimelineStore.setState({ panBy });
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 0);

    fireEvent.pointerDown(canvas, { clientX: 400, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 403, pointerId: 1 });
    fireEvent.pointerUp(canvas, { clientX: 403, pointerId: 1 });

    expect(panBy).not.toHaveBeenCalled();
    expect(seek).toHaveBeenCalledWith(at(403));
  });
});

describe('drag to pan', () => {
  it('moves the window against the drag, measured from where the drag began', () => {
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 0);

    fireEvent.pointerDown(canvas, { clientX: 400, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 500, pointerId: 1 });
    // Dragging right by a tenth of the width shows the minute before.
    expect(useTimelineStore.getState().windowStartMs).toBe(START - 60_000);

    fireEvent.pointerMove(canvas, { clientX: 550, pointerId: 1 });
    expect(useTimelineStore.getState().windowStartMs).toBe(START - 90_000);
    expect(useTimelineStore.getState().followingLive).toBe(false);
  });

  it('does not seek when the drag ends', () => {
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 0);

    fireEvent.pointerDown(canvas, { clientX: 400, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 300, pointerId: 1 });
    fireEvent.pointerUp(canvas, { clientX: 300, pointerId: 1 });

    expect(useTimelineStore.getState().windowStartMs).toBe(START + 60_000);
    expect(seek).not.toHaveBeenCalled();
  });

  it('ignores a second pointer moving while the first holds the timeline', () => {
    const panBy = vi.fn<(deltaMs: number) => void>();
    useTimelineStore.setState({ panBy });
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 0);

    fireEvent.pointerDown(canvas, { clientX: 400, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 600, pointerId: 2 });

    expect(panBy).not.toHaveBeenCalled();
  });

  it('does nothing when the pointer only passes over', () => {
    const panBy = vi.fn<(deltaMs: number) => void>();
    useTimelineStore.setState({ panBy });
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 0);

    fireEvent.pointerMove(canvas, { clientX: 100, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 600, pointerId: 1 });

    expect(panBy).not.toHaveBeenCalled();
    expect(seek).not.toHaveBeenCalled();
  });
});

describe('scroll to zoom', () => {
  let zoomTo: Mock<(spanMs: number, anchorMs?: number | null) => void>;

  beforeEach(() => {
    zoomTo = vi.fn<(spanMs: number, anchorMs?: number | null) => void>();
    useTimelineStore.setState({ zoomTo });
  });

  it('zooms out on a scroll down, holding the moment under the pointer', () => {
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 100);
    fireEvent.wheel(canvas, { deltaY: 120, clientX: 350 });
    expect(zoomTo).toHaveBeenCalledWith(SPAN * 1.25, at(250));
  });

  it('zooms in on a scroll up, holding the moment under the pointer', () => {
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 100);
    fireEvent.wheel(canvas, { deltaY: -120, clientX: 900 });
    expect(zoomTo).toHaveBeenCalledWith(SPAN * 0.8, at(800));
  });

  it('ignores a purely sideways scroll', () => {
    const { canvas } = renderScrubber();
    placeCanvas(canvas, 100);
    fireEvent.wheel(canvas, { deltaY: 0, deltaX: 50, clientX: 350 });
    expect(zoomTo).not.toHaveBeenCalled();
  });
});
