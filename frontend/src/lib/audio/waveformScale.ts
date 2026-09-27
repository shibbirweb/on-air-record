/**
 * How the Broadcast card's oscilloscope scales what it draws.
 *
 * A microphone records far below full scale: ordinary speech in a room is a few percent of it, which at
 * true size is a trace under a pixel tall, and reads as nothing on screen. So the drawing, never the sound,
 * is magnified until the loudest recent sample fills most of the height. It shrinks at once when something
 * louder arrives, so a clap never clips, and grows back slowly, so the picture does not pump with every
 * syllable. Scaling to the signal also takes the volume slider out of the picture, since the analyser sits
 * after it.
 */

/** The loudest recent sample is drawn at this fraction of the half height. */
export const TARGET_FILL = 0.8;

/** The most a quiet signal is magnified, so room noise stays a faint ripple rather than filling the box. */
export const MAX_DISPLAY_GAIN = 16;

/** Share of the way to a larger gain taken each frame: about a second to settle at 60 frames a second. */
export const GROW_RATE = 0.05;

/** Half the thickness of the thinnest stroke, in pixels, so silence draws as a line and not as nothing. */
export const MIN_HALF_STROKE_PX = 0.5;

/** The display gain for this frame, given the last one and the loudest sample the analyser now holds. */
export function nextDisplayGain(current: number, peak: number): number {
  const loudest = Number.isFinite(peak) ? Math.abs(peak) : 0;
  const wanted = loudest > 0 ? Math.min(TARGET_FILL / loudest, MAX_DISPLAY_GAIN) : MAX_DISPLAY_GAIN;
  const from = Number.isFinite(current) && current > 0 ? current : 1;
  if (wanted <= from) {
    return wanted;
  }
  return Math.min(from + (wanted - from) * GROW_RATE, wanted);
}
