/**
 * The CCTV style timeline.
 *
 * One canvas draws four layers: the coverage bands that say what was recorded, the waveform envelope, the
 * time grid, and the moving parts (playhead, live edge, hover cursor). Canvas rather than DOM because the
 * playhead moves sixty times a second and the envelope can be a thousand bars, and a thousand divs being
 * repositioned every frame is exactly the kind of thing that makes a page feel heavy.
 *
 * The component reads the playhead through a callback rather than a prop so that a moving playhead never
 * triggers a React render. Only the things that genuinely change the layout, the window and the fetched
 * data, come from the store.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

import { useAnimationFrame } from '@/hooks/useAnimationFrame';
import { formatClock } from '@/lib/format';
import { chooseTickStepMs, tickTimestamps, timeToX, xToTime } from '@/lib/timelineGeometry';
import type { TimelineWindow } from '@/lib/timelineGeometry';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

type TimelineScrubberProps = {
  /** Reads the current playhead straight off the audio clock. */
  getPlayheadMs: () => number | null;
  className?: string;
};

/** Movement in pixels before a press counts as a pan rather than a click. */
const DRAG_THRESHOLD_PX = 4;

const HEIGHT = 132;
const RULER_HEIGHT = 22;

function readCssColor(element: HTMLElement, token: string, fallback: string): string {
  const value = getComputedStyle(element).getPropertyValue(token).trim();
  return value.length > 0 ? value : fallback;
}

export function TimelineScrubber({ getPlayheadMs, className }: TimelineScrubberProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [hoverMs, setHoverMs] = useState<number | null>(null);

  const dragState = useRef<{ pointerId: number; startX: number; startWindowMs: number; moved: boolean } | null>(
    null,
  );

  const windowStartMs = useTimelineStore((state) => state.windowStartMs);
  const spanMs = useTimelineStore((state) => state.spanMs);
  const peaks = useTimelineStore((state) => state.peaks);
  const range = useTimelineStore((state) => state.range);
  const panBy = useTimelineStore((state) => state.panBy);
  const setSpan = useTimelineStore((state) => state.setSpan);
  const seek = useTransportStore((state) => state.seek);
  // Where the listener asked to be. Set the moment the timeline is clicked, long before the audio graph
  // has anything to say about it.
  const requestedPositionMs = useTransportStore((state) => state.requestedPositionMs);

  // The draw loop runs outside React, so everything it reads is mirrored into refs and refreshed after
  // each render. Without this the loop would close over the values from the render that started it.
  const viewRef = useRef({ windowStartMs, spanMs });
  const dataRef = useRef({ peaks, range });
  const hoverRef = useRef<number | null>(null);
  const cueRef = useRef<number | null>(requestedPositionMs);

  useEffect(() => {
    viewRef.current = { windowStartMs, spanMs };
    dataRef.current = { peaks, range };
    hoverRef.current = hoverMs;
    cueRef.current = requestedPositionMs;
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

    const view: TimelineWindow = {
      startMs: viewRef.current.windowStartMs,
      spanMs: viewRef.current.spanMs,
      width,
    };

    const colours = {
      grid: readCssColor(container, '--border', '#333'),
      text: readCssColor(container, '--muted-foreground', '#888'),
      coverage: readCssColor(container, '--coverage', '#444'),
      wave: readCssColor(container, '--wave', '#f0a'),
      live: readCssColor(container, '--live', '#f33'),
      playhead: readCssColor(container, '--foreground', '#fff'),
      surface: readCssColor(container, '--card', '#111'),
    };

    const waveTop = RULER_HEIGHT;
    const waveHeight = HEIGHT - RULER_HEIGHT;
    const centreY = waveTop + waveHeight / 2;

    // Layer 1: coverage. Everything outside a band was never recorded, and showing that plainly is the
    // difference between "there is silence here" and "there is nothing here".
    context.fillStyle = colours.coverage;
    for (const band of dataRef.current.range?.coverage ?? []) {
      const left = timeToX(band.startMs, view);
      const right = timeToX(band.endMs, view);
      if (right < 0 || left > width) {
        continue;
      }
      context.globalAlpha = 0.35;
      context.fillRect(left, waveTop, Math.max(right - left, 1), waveHeight);
      context.globalAlpha = 1;
    }

    // Layer 2: the waveform envelope, drawn symmetrically around the centre line.
    const envelope = dataRef.current.peaks;
    if (envelope && envelope.peaks.length > 0) {
      const bucketCount = envelope.peaks.length;
      const bucketSpanMs = (envelope.toMs - envelope.fromMs) / bucketCount;

      context.fillStyle = colours.wave;
      for (let x = 0; x < width; x += 1) {
        const timestampMs = xToTime(x, view);
        const index = Math.floor((timestampMs - envelope.fromMs) / bucketSpanMs);
        if (index < 0 || index >= bucketCount) {
          continue;
        }
        const amplitude = envelope.peaks[index] / 255;
        if (amplitude <= 0) {
          continue;
        }
        const half = Math.max((amplitude * waveHeight) / 2, 0.5);
        context.fillRect(x, centreY - half, 1, half * 2);
      }
    }

    // Layer 3: the time grid and its labels.
    const stepMs = chooseTickStepMs(view);
    context.strokeStyle = colours.grid;
    context.fillStyle = colours.text;
    context.font = '10px ui-monospace, SFMono-Regular, Menlo, monospace';
    context.textBaseline = 'middle';
    context.lineWidth = 1;

    for (const tick of tickTimestamps(view, stepMs)) {
      const x = Math.round(timeToX(tick, view)) + 0.5;
      context.globalAlpha = 0.5;
      context.beginPath();
      context.moveTo(x, RULER_HEIGHT);
      context.lineTo(x, HEIGHT);
      context.stroke();
      context.globalAlpha = 1;
      context.fillText(formatClock(tick), x + 4, RULER_HEIGHT / 2);
    }

    // Layer 4: the moving parts.
    const liveEdge = dataRef.current.range?.liveEdgeMs ?? null;
    if (liveEdge !== null) {
      const x = timeToX(liveEdge, view);
      if (x >= 0 && x <= width) {
        context.strokeStyle = colours.live;
        context.lineWidth = 2;
        context.beginPath();
        context.moveTo(x, waveTop);
        context.lineTo(x, HEIGHT);
        context.stroke();
      }
    }

    const hover = hoverRef.current;
    if (hover !== null) {
      const x = timeToX(hover, view);
      context.strokeStyle = colours.text;
      context.globalAlpha = 0.6;
      context.lineWidth = 1;
      context.setLineDash([3, 3]);
      context.beginPath();
      context.moveTo(x, waveTop);
      context.lineTo(x, HEIGHT);
      context.stroke();
      context.setLineDash([]);
      context.globalAlpha = 1;
    }

    // The audio clock is authoritative once it is running. Before that, and in the gap right after a seek
    // while the jitter buffer refills, fall back to the position that was asked for. Without the fallback
    // clicking the timeline while stopped moves the server but draws nothing, which reads as a dead click.
    const audioPlayheadMs = getPlayheadMs();
    const markerMs = audioPlayheadMs ?? cueRef.current;

    if (markerMs !== null) {
      const x = timeToX(markerMs, view);
      if (x >= -8 && x <= width + 8) {
        // Filled head means audio is actually coming out here. Hollow means cued and waiting for play.
        const playing = audioPlayheadMs !== null;

        context.globalAlpha = playing ? 1 : 0.85;
        context.strokeStyle = colours.playhead;
        context.lineWidth = 2;
        context.beginPath();
        context.moveTo(x, waveTop);
        context.lineTo(x, HEIGHT);
        context.stroke();

        context.beginPath();
        context.moveTo(x - 5, waveTop);
        context.lineTo(x + 5, waveTop);
        context.lineTo(x, waveTop + 7);
        context.closePath();

        if (playing) {
          context.fillStyle = colours.playhead;
          context.fill();
        } else {
          context.fillStyle = colours.surface;
          context.fill();
          context.lineWidth = 1.5;
          context.stroke();
        }

        context.globalAlpha = 1;
      }
    }
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
    return xToTime(event.clientX - bounds.left, {
      startMs: viewRef.current.windowStartMs,
      spanMs: viewRef.current.spanMs,
      width: bounds.width,
    });
  };

  const onPointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    dragState.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWindowMs: viewRef.current.windowStartMs,
      moved: false,
    };
  };

  const onPointerMove = (event: React.PointerEvent<HTMLCanvasElement>) => {
    setHoverMs(pointerTime(event));

    const drag = dragState.current;
    if (!drag || drag.pointerId !== event.pointerId) {
      return;
    }

    const deltaX = event.clientX - drag.startX;
    if (!drag.moved && Math.abs(deltaX) < DRAG_THRESHOLD_PX) {
      return;
    }

    drag.moved = true;
    const bounds = event.currentTarget.getBoundingClientRect();
    const deltaMs = -(deltaX / bounds.width) * viewRef.current.spanMs;
    // Pan from the position the drag started at, so a slow drag does not accumulate rounding error.
    const target = drag.startWindowMs + deltaMs;
    panBy(target - viewRef.current.windowStartMs);
  };

  const onPointerUp = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const drag = dragState.current;
    dragState.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);

    // A press that did not move is a seek. A press that moved was a pan, and seeking at the end of it
    // would be infuriating.
    if (drag && !drag.moved) {
      seek(pointerTime(event));
    }
  };

  const onWheel = (event: React.WheelEvent<HTMLCanvasElement>) => {
    if (event.deltaY === 0) {
      return;
    }
    const factor = event.deltaY > 0 ? 1.25 : 0.8;
    setSpan(viewRef.current.spanMs * factor);
  };

  return (
    <div ref={containerRef} className={className}>
      <canvas
        ref={canvasRef}
        className="w-full cursor-crosshair touch-none rounded-lg border bg-card select-none"
        style={{ height: HEIGHT }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerLeave={() => setHoverMs(null)}
        onWheel={onWheel}
      />
      <div className="text-muted-foreground mt-1.5 flex items-center justify-between text-xs tabular">
        <span>{formatClock(windowStartMs)}</span>
        <span className="text-[11px]">
          {hoverMs === null ? 'click to seek, drag to pan, scroll to zoom' : formatClock(hoverMs)}
        </span>
        <span>{formatClock(windowStartMs + spanMs)}</span>
      </div>
    </div>
  );
}
