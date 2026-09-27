/**
 * A fuller record of canvas drawing than `recordCanvases` keeps on its own, for the layers of the timeline
 * and the waveform that are lines and text rather than filled rectangles: the grid, the live edge, the
 * playhead, the bookmark poles and the oscilloscope trace.
 *
 * It sits on top of `recordCanvases`, whose context it wraps, so filled rectangles still land in the same
 * `paints` list and the animation frames are still cranked by hand. On top of that it keeps every stroked
 * or filled path with the points it was built from, every piece of text, and every stroked rectangle,
 * each with the colour, width, transparency and dash in force at the time.
 */

import { vi } from 'vitest';

import { recordCanvases } from '@/test/canvas';
import type { CanvasRecorder } from '@/test/canvas';

export type Point = { x: number; y: number };

export type PathPaint = {
  kind: 'stroke' | 'fill';
  points: Point[];
  style: string;
  lineWidth: number;
  alpha: number;
  dashed: boolean;
};

export type TextPaint = {
  text: string;
  x: number;
  y: number;
  fillStyle: string;
};

export type RectStroke = {
  x: number;
  y: number;
  width: number;
  height: number;
  strokeStyle: string;
  alpha: number;
};

export type DrawingRecorder = CanvasRecorder & {
  paths: PathPaint[];
  texts: TextPaint[];
  rectStrokes: RectStroke[];
  /** Forget everything drawn so far, so an assertion sees one frame only. */
  clear: () => void;
};

/** Record every canvas as if it were `width` CSS pixels wide, lines and text included. */
export function recordDrawing(width: number): DrawingRecorder {
  const canvas = recordCanvases(width);
  // The base context `recordCanvases` hands out. Every canvas shares it, so one is enough to wrap.
  const base = document.createElement('canvas').getContext('2d') as unknown as Record<string, unknown>;
  // The base answers any property it has never been given with a no-op function, so the two a path
  // snapshot reads are given real starting values, as a browser's context has.
  base.strokeStyle = '#000';
  base.lineWidth = 1;

  const paths: PathPaint[] = [];
  const texts: TextPaint[] = [];
  const rectStrokes: RectStroke[] = [];
  let current: Point[] = [];
  let dashed = false;

  const snapshot = (kind: 'stroke' | 'fill'): PathPaint => ({
    kind,
    points: [...current],
    style: String(kind === 'stroke' ? base.strokeStyle : base.fillStyle),
    lineWidth: Number(base.lineWidth),
    alpha: Number(base.globalAlpha),
    dashed,
  });

  const overrides: Record<string, (...args: never[]) => unknown> = {
    beginPath: () => {
      current = [];
    },
    moveTo: (x: number, y: number) => {
      current.push({ x, y });
    },
    lineTo: (x: number, y: number) => {
      current.push({ x, y });
    },
    closePath: () => undefined,
    stroke: () => {
      paths.push(snapshot('stroke'));
    },
    fill: () => {
      paths.push(snapshot('fill'));
    },
    setLineDash: (segments: number[]) => {
      dashed = segments.length > 0;
    },
    fillText: (text: string, x: number, y: number) => {
      texts.push({ text, x, y, fillStyle: String(base.fillStyle) });
    },
    strokeRect: (x: number, y: number, rectWidth: number, height: number) => {
      rectStrokes.push({
        x,
        y,
        width: rectWidth,
        height,
        strokeStyle: String(base.strokeStyle),
        alpha: Number(base.globalAlpha),
      });
    },
  };

  const context = new Proxy(base, {
    get(target, property) {
      if (typeof property === 'string' && property in overrides) {
        return overrides[property];
      }
      return target[property as string];
    },
    set(target, property, value) {
      target[property as string] = value;
      return true;
    },
  });

  vi.mocked(HTMLCanvasElement.prototype.getContext).mockImplementation(
    () => context as unknown as CanvasRenderingContext2D,
  );

  return {
    ...canvas,
    paths,
    texts,
    rectStrokes,
    clear: () => {
      canvas.paints.length = 0;
      paths.length = 0;
      texts.length = 0;
      rectStrokes.length = 0;
    },
  };
}

/** A vertical line from `top` to `bottom` at `x`, as the timeline draws its markers. */
export function isVerticalLine(path: PathPaint, x: number, top: number, bottom: number): boolean {
  return (
    path.points.length === 2 &&
    path.points[0].x === x &&
    path.points[1].x === x &&
    path.points[0].y === top &&
    path.points[1].y === bottom
  );
}
