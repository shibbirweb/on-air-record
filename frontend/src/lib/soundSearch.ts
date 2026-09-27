/**
 * Where the next and previous sound buttons search from, and what they say when there is nothing that way.
 *
 * Kept out of the transport bar so every combination is tested without a browser: the rule has four
 * sources in order of preference, and getting the order wrong is exactly the kind of mistake that looks
 * fine on the one path somebody happens to click.
 */

export type SoundSearchDirection = 'forward' | 'backward';

export type SoundSearchContext = {
  /** What is being heard now, from the audio clock, or `null` when nothing plays. */
  playheadMs: number | null;
  /** The moment cued by a click or a jump, or `null` when nothing is cued. */
  cuedMs: number | null;
  /** Left edge of the visible timeline. */
  windowStartMs: number;
  /** Whether the timeline is scrolling with the live edge. */
  followingLive: boolean;
  /** The newest moment being recorded, or `null` when nothing is. */
  liveEdgeMs: number | null;
  nowMs: number;
};

/**
 * The position a search starts from.
 *
 * What is heard comes first, then what is cued: the same position the transport readout shows. With
 * neither, where the timeline is looking, because somebody who has scrolled to last night and presses next
 * means the first sound there, not "after now". Only a timeline following live searches from the live
 * edge, and from the clock when nothing is recording either.
 */
export function soundSearchFrom(context: SoundSearchContext): number {
  if (context.playheadMs !== null) {
    return context.playheadMs;
  }
  if (context.cuedMs !== null) {
    return context.cuedMs;
  }
  if (!context.followingLive) {
    return context.windowStartMs;
  }
  return context.liveEdgeMs ?? context.nowMs;
}

/** What to say when a search finds nothing that way. */
export function noSoundNotice(direction: SoundSearchDirection): string {
  return direction === 'forward'
    ? 'No later sound recorded yet. Go live to hear what is happening now.'
    : 'No earlier sound in the recordings.';
}
