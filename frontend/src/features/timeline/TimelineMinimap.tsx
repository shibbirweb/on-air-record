/**
 * Twenty four hour overview of the day, with the main timeline's window drawn on it.
 *
 * This is the overview half of an overview plus detail pair. The scrubber above shows minutes in detail
 * and loses all sense of where those minutes sit; this shows the whole day and answers "where am I" at a
 * glance. Drag the bright window to move the detailed view, or click anywhere to jump there.
 *
 * The frame is a calendar day rather than a rolling twenty four hours ending now. A minimap that slides
 * as you pan gives you nothing fixed to orient against, which defeats the purpose.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

import { useAnimationFrame } from '@/hooks/useAnimationFrame';
import { dayLabel, localDayId } from '@/lib/day';
import { formatClock } from '@/lib/format';
import { timeToX, xToTime } from '@/lib/timelineGeometry';
import type { TimelineWindow } from '@/lib/timelineGeometry';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

type TimelineMinimapProps = {
  getPlayheadMs: () => number | null;
  className?: string;
};

const HEIGHT = 54;
/** Strip along the top holding the hour labels. */
const LABEL_HEIGHT = 15;
/** Narrower than this and the window is impossible to grab, so it is drawn at this minimum. */
const MIN_VIEWPORT_PX = 10;

function readCssColor(element: HTMLElement, token: string, fallback: string): string {
  const value = getComputedStyle(element).getPropertyValue(token).trim();
  return value.length > 0 ? value : fallback;
}

export function TimelineMinimap({ getPlayheadMs, className }: TimelineMinimapProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [grabbing, setGrabbing] = useState(false);

  /** Offset from the window start to where the pointer took hold, so the window does not jump on grab. */
  const dragRef = useRef<{ pointerId: number; grabOffsetMs: number } | null>(null);

  const windowStartMs = useTimelineStore((state) => state.windowStartMs);
  const spanMs = useTimelineStore((state) => state.spanMs);
  const dayPeaks = useTimelineStore((state) => state.dayPeaks);
  const range = useTimelineStore((state) => state.range);
  const scrollTo = useTimelineStore((state) => state.scrollTo);
  const minimapStartMs = useTimelineStore((state) => state.minimapWindow().startMs);
  const minimapEndMs = useTimelineStore((state) => state.minimapWindow().endMs);
  const requestedPositionMs = useTransportStore((state) => state.requestedPositionMs);
  const bookmarks = useBookmarkStore((state) => state.bookmarks);

  // Everything the draw loop reads, refreshed after each render. The loop runs outside React.
  const stateRef = useRef({
    windowStartMs,
    spanMs,
    minimapStartMs,
    minimapEndMs,
    dayPeaks,
    range,
    requestedPositionMs,
    bookmarks,
  });

  useEffect(() => {
    stateRef.current = {
      windowStartMs,
      spanMs,
      minimapStartMs,
      minimapEndMs,
      dayPeaks,
      range,
      requestedPositionMs,
      bookmarks,
    };
  });

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) {
      return;
    }

    const ratio = window.devicePixelRatio || 1;
    const width = container.clientWidth;
    if (width === 0) {
      return;
    }

    if (canvas.width !== Math.floor(width * ratio) || canvas.height !== Math.floor(HEIGHT * ratio)) {
      canvas.width = Math.floor(width * ratio);
      canvas.height = Math.floor(HEIGHT * ratio);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${HEIGHT}px`;
    }

    const context = canvas.getContext('2d');
    if (!context) {
      return;
    }

    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, HEIGHT);

    const current = stateRef.current;
    const view: TimelineWindow = {
      startMs: current.minimapStartMs,
      spanMs: current.minimapEndMs - current.minimapStartMs,
      width,
    };

    const colours = {
      grid: readCssColor(container, '--border', '#333'),
      text: readCssColor(container, '--muted-foreground', '#888'),
      coverage: readCssColor(container, '--coverage', '#444'),
      wave: readCssColor(container, '--wave', '#f0a'),
      live: readCssColor(container, '--live', '#f33'),
      marker: readCssColor(container, '--foreground', '#fff'),
      bookmark: readCssColor(container, '--primary', '#e8a33d'),
    };

    const barTop = LABEL_HEIGHT;
    const barHeight = HEIGHT - LABEL_HEIGHT;
    const centreY = barTop + barHeight / 2;

    // Hour grid, labelled every six hours. Enough structure to read the time of day off the bar without
    // crowding fifty four pixels with text.
    context.font = '9px ui-monospace, SFMono-Regular, Menlo, monospace';
    context.textBaseline = 'middle';
    for (let hour = 0; hour <= 24; hour += 1) {
      const x = Math.round((hour / 24) * width) + 0.5;
      const major = hour % 6 === 0;

      context.strokeStyle = colours.grid;
      context.globalAlpha = major ? 0.7 : 0.3;
      context.beginPath();
      context.moveTo(x, major ? barTop : barTop + barHeight * 0.65);
      context.lineTo(x, HEIGHT);
      context.stroke();
      context.globalAlpha = 1;

      if (major && hour < 24) {
        context.fillStyle = colours.text;
        context.fillText(`${hour.toString().padStart(2, '0')}:00`, x + 3, LABEL_HEIGHT / 2);
      }
    }

    // Coverage, so the empty stretches of the day are obvious.
    context.fillStyle = colours.coverage;
    for (const band of current.range?.coverage ?? []) {
      const left = timeToX(band.startMs, view);
      const right = timeToX(band.endMs, view);
      if (right < 0 || left > width) {
        continue;
      }
      context.globalAlpha = 0.4;
      context.fillRect(left, barTop, Math.max(right - left, 1), barHeight);
      context.globalAlpha = 1;
    }

    // The day's envelope, drawn symmetrically about the middle of the bar.
    const envelope = current.dayPeaks;
    if (envelope && envelope.peaks.length > 0) {
      const bucketCount = envelope.peaks.length;
      const bucketSpanMs = (envelope.toMs - envelope.fromMs) / bucketCount;

      context.fillStyle = colours.wave;
      for (let x = 0; x < width; x += 1) {
        const index = Math.floor((xToTime(x, view) - envelope.fromMs) / bucketSpanMs);
        if (index < 0 || index >= bucketCount) {
          continue;
        }
        const amplitude = envelope.peaks[index] / 255;
        if (amplitude <= 0) {
          continue;
        }
        const half = Math.max((amplitude * barHeight * 0.8) / 2, 0.5);
        context.fillRect(x, centreY - half, 1, half * 2);
      }
    }

    // Tick marks only at this size. The label would not fit, and the point here is to show where in the
    // day the marked moments fall so they can be scrolled to.
    context.fillStyle = colours.bookmark;
    for (const bookmark of current.bookmarks) {
      const x = timeToX(bookmark.timestampMs, view);
      if (x < 0 || x > width) {
        continue;
      }
      context.fillRect(x - 1, barTop, 2, 5);
    }

    const liveEdgeMs = current.range?.liveEdgeMs ?? null;
    if (liveEdgeMs !== null) {
      const x = timeToX(liveEdgeMs, view);
      if (x >= 0 && x <= width) {
        context.strokeStyle = colours.live;
        context.lineWidth = 1.5;
        context.beginPath();
        context.moveTo(x, barTop);
        context.lineTo(x, HEIGHT);
        context.stroke();
      }
    }

    const markerMs = getPlayheadMs() ?? current.requestedPositionMs;
    if (markerMs !== null) {
      const x = timeToX(markerMs, view);
      if (x >= 0 && x <= width) {
        context.strokeStyle = colours.marker;
        context.lineWidth = 1.5;
        context.globalAlpha = 0.9;
        context.beginPath();
        context.moveTo(x, barTop);
        context.lineTo(x, HEIGHT);
        context.stroke();
        context.globalAlpha = 1;
      }
    }

    // The viewport last, over everything.
    //
    // Highlighting the inside rather than dimming the outside, because the panel and page backgrounds are
    // deliberately close in both themes: a wash of the background colour over the rest of the bar is
    // almost invisible, while a wash of the foreground colour reads immediately whichever theme is on.
    const viewportLeft = timeToX(current.windowStartMs, view);
    const viewportRight = timeToX(current.windowStartMs + current.spanMs, view);
    const clampedLeft = Math.max(Math.min(viewportLeft, width - MIN_VIEWPORT_PX), 0);
    const clampedRight = Math.min(Math.max(viewportRight, clampedLeft + MIN_VIEWPORT_PX), width);
    const viewportWidth = Math.max(clampedRight - clampedLeft, MIN_VIEWPORT_PX);

    context.fillStyle = colours.marker;
    context.globalAlpha = 0.16;
    context.fillRect(clampedLeft, barTop, viewportWidth, barHeight);
    context.globalAlpha = 1;

    context.strokeStyle = colours.marker;
    context.lineWidth = 1;
    context.globalAlpha = 0.9;
    context.strokeRect(clampedLeft + 0.5, barTop + 0.5, viewportWidth - 1, barHeight - 1);
    context.globalAlpha = 1;

    // Grab handles, so the window reads as something you can take hold of even when it is only a few
    // pixels wide, which it is whenever a short window is shown against a whole day.
    context.fillStyle = colours.marker;
    context.fillRect(clampedLeft, barTop + 2, 2, barHeight - 4);
    context.fillRect(clampedLeft + viewportWidth - 2, barTop + 2, 2, barHeight - 4);
  }, [getPlayheadMs]);

  useAnimationFrame(draw);

  useEffect(() => {
    const observer = new ResizeObserver(() => draw());
    if (containerRef.current) {
      observer.observe(containerRef.current);
    }
    return () => observer.disconnect();
  }, [draw]);

  const pointerTime = (event: React.PointerEvent<HTMLCanvasElement>): number => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const current = stateRef.current;
    return xToTime(event.clientX - bounds.left, {
      startMs: current.minimapStartMs,
      spanMs: current.minimapEndMs - current.minimapStartMs,
      width: bounds.width,
    });
  };

  const onPointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);

    const current = stateRef.current;
    const at = pointerTime(event);
    const insideViewport =
      at >= current.windowStartMs && at <= current.windowStartMs + current.spanMs;

    // Grabbing inside keeps the window where it is under the pointer. Landing outside centres it there
    // first, so a click is a jump and the drag then continues naturally from the middle.
    const grabOffsetMs = insideViewport ? at - current.windowStartMs : current.spanMs / 2;

    dragRef.current = { pointerId: event.pointerId, grabOffsetMs };
    setGrabbing(true);
    scrollTo(at - grabOffsetMs);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) {
      return;
    }
    scrollTo(pointerTime(event) - drag.grabOffsetMs);
  };

  const endDrag = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (dragRef.current?.pointerId === event.pointerId) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragRef.current = null;
    setGrabbing(false);
  };

  const dayId = localDayId(minimapStartMs);

  return (
    <div ref={containerRef} className={className}>
      <canvas
        ref={canvasRef}
        className={`w-full touch-none rounded-lg border bg-card select-none ${
          grabbing ? 'cursor-grabbing' : 'cursor-grab'
        }`}
        style={{ height: HEIGHT }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      />
      <div className="text-muted-foreground mt-1 flex items-center justify-between text-[11px] tabular">
        <span>{formatClock(minimapStartMs)}</span>
        <span>
          {dayLabel(dayId, null, null)} overview, drag the window to move the timeline
        </span>
        <span>{formatClock(minimapEndMs - 1)}</span>
      </div>
    </div>
  );
}
