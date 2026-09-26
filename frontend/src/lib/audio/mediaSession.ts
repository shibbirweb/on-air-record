/**
 * Lock screen and notification controls, through the Media Session API.
 *
 * Once the output plays through a media element, a phone shows the page on its lock screen like any
 * music app. This fills in what it shows and wires its buttons to the transport, so the broadcast can be
 * paused and resumed without unlocking. Everything here is optional: a browser without the API, or
 * without \`MediaMetadata\`, simply shows no controls.
 */

import type { StreamMode } from '@/api/types';

export type NowPlaying = {
  title: string;
  artist: string;
  album: string;
};

/**
 * What the lock screen says. The recorder's address goes in the album line, because two recorders on one
 * network are otherwise indistinguishable from the lock screen.
 */
export function describeNowPlaying(mode: StreamMode, host: string): NowPlaying {
  return {
    title: mode === 'playback' ? 'Listening back' : 'Live',
    artist: 'On Air Record',
    album: host,
  };
}

/** Icons for the lock screen. Raster, because phones do not reliably draw SVG artwork there. */
const ARTWORK: MediaImage[] = [
  { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
  { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
];

/** Show what is playing and whether it is playing. Does nothing where the API is missing. */
export function showNowPlaying(
  session: MediaSession | undefined,
  playing: boolean,
  mode: StreamMode,
  host: string,
): void {
  if (!session) {
    return;
  }
  if (typeof MediaMetadata !== 'undefined') {
    session.metadata = new MediaMetadata({ ...describeNowPlaying(mode, host), artwork: ARTWORK });
  }
  session.playbackState = playing ? 'playing' : 'paused';
  // A broadcast has no end. Saying so stops Chrome drawing a progress bar from the ten second silent clip
  // that keeps its notification alive on Android.
  try {
    session.setPositionState?.({ duration: Number.POSITIVE_INFINITY, playbackRate: 1, position: 0 });
  } catch {
    // An older browser that wants a finite duration. It shows no position then, which is fine.
  }
}

export type SessionActions = {
  play: () => void;
  pause: () => void;
};

/**
 * Wire the lock screen buttons. Returns a function that unwires them. Play and pause only: the broadcast
 * has no next track, and seeking from a lock screen with no timeline in view is more confusing than useful.
 */
export function bindSessionActions(session: MediaSession | undefined, actions: SessionActions): () => void {
  if (!session) {
    return () => undefined;
  }
  const bindings: [MediaSessionAction, (() => void) | null][] = [
    ['play', actions.play],
    ['pause', actions.pause],
    // Some phones show stop on the notification; for a broadcast it means the same as pause.
    ['stop', actions.pause],
  ];
  for (const [action, handler] of bindings) {
    try {
      session.setActionHandler(action, handler);
    } catch {
      // This browser does not support that action. The others still work.
    }
  }
  return () => {
    for (const [action] of bindings) {
      try {
        session.setActionHandler(action, null);
      } catch {
        // Never bound. Nothing to undo.
      }
    }
  };
}
