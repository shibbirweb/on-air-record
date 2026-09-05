/**
 * Pure geometry for the timeline canvas.
 *
 * Kept out of the component so the mapping between a moment in time and a pixel can be reasoned about,
 * and tested, without a DOM. Every drawing and hit testing decision in the scrubber goes through these
 * two functions, which is what stops the playhead and the waveform from disagreeing by a pixel.
 */

export type TimelineWindow = {
  startMs: number;
  spanMs: number;
  width: number;
};

/** Horizontal pixel for a moment in time. */
export function timeToX(timestampMs: number, view: TimelineWindow): number {
  if (view.spanMs <= 0) {
    return 0;
  }
  return ((timestampMs - view.startMs) / view.spanMs) * view.width;
}

/** The moment a horizontal pixel represents. */
export function xToTime(x: number, view: TimelineWindow): number {
  if (view.width <= 0) {
    return view.startMs;
  }
  return view.startMs + (x / view.width) * view.spanMs;
}

/** Nice tick intervals, from one second to one day. */
const TICK_STEPS_MS = [
  1_000, 5_000, 10_000, 15_000, 30_000,
  60_000, 5 * 60_000, 10 * 60_000, 15 * 60_000, 30 * 60_000,
  3_600_000, 3 * 3_600_000, 6 * 3_600_000, 12 * 3_600_000,
  24 * 3_600_000,
];

/**
 * Pick a tick interval that puts labels roughly `targetSpacingPx` apart.
 *
 * Snapping to a familiar step matters more than hitting the target exactly: a gridline every 37 seconds
 * is unreadable even when it is perfectly spaced, while one every 30 seconds can be read at a glance.
 */
export function chooseTickStepMs(view: TimelineWindow, targetSpacingPx = 110): number {
  if (view.width <= 0) {
    return TICK_STEPS_MS[TICK_STEPS_MS.length - 1];
  }

  const wanted = (targetSpacingPx / view.width) * view.spanMs;
  return TICK_STEPS_MS.find((step) => step >= wanted) ?? TICK_STEPS_MS[TICK_STEPS_MS.length - 1];
}

/** Tick timestamps covering the window, aligned to whole multiples of the step. */
export function tickTimestamps(view: TimelineWindow, stepMs: number): number[] {
  if (stepMs <= 0) {
    return [];
  }

  const first = Math.ceil(view.startMs / stepMs) * stepMs;
  const ticks: number[] = [];
  // A hard cap, because a pathological span with a tiny step would otherwise try to draw millions.
  for (let value = first; value <= view.startMs + view.spanMs && ticks.length < 512; value += stepMs) {
    ticks.push(value);
  }
  return ticks;
}
