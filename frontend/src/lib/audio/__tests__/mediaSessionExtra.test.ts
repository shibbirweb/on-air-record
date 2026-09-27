/**
 * Lock screen cases `mediaSession.test.ts` leaves out: the position state that stops Chrome drawing a
 * progress bar from the silent keeper clip, an older browser that refuses an infinite duration, the
 * artwork, and unwiring on a browser that throws when an action is cleared.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { bindSessionActions, describeNowPlaying, showNowPlaying } from '../mediaSession';

function fakeSession() {
  return {
    metadata: null as unknown,
    playbackState: 'none' as MediaSessionPlaybackState,
    positions: [] as unknown[],
    setPositionState(state: unknown) {
      this.positions.push(state);
    },
    handlers: new Map<string, (() => void) | null>(),
    setActionHandler(action: string, handler: (() => void) | null) {
      this.handlers.set(action, handler);
    },
  };
}

beforeEach(() => {
  vi.stubGlobal(
    'MediaMetadata',
    class {
      readonly init: unknown;
      constructor(init: unknown) {
        this.init = init;
      }
    },
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('showNowPlaying, position and artwork', () => {
  it('tells the browser the broadcast has no end', () => {
    const session = fakeSession();
    showNowPlaying(session as unknown as MediaSession, true, 'live', 'recorder.local');
    expect(session.positions).toEqual([{ duration: Number.POSITIVE_INFINITY, playbackRate: 1, position: 0 }]);
  });

  it('carries on when the browser wants a finite duration', () => {
    const session = fakeSession();
    session.setPositionState = () => {
      throw new TypeError('duration must be finite');
    };
    expect(() => showNowPlaying(session as unknown as MediaSession, false, 'live', 'recorder.local')).not.toThrow();
    expect(session.playbackState).toBe('paused');
  });

  it('works where setPositionState does not exist', () => {
    const session = fakeSession() as Partial<ReturnType<typeof fakeSession>>;
    delete session.setPositionState;
    expect(() => showNowPlaying(session as unknown as MediaSession, true, 'live', 'recorder.local')).not.toThrow();
  });

  it('offers raster icons at two sizes, since phones do not reliably draw SVG there', () => {
    const session = fakeSession();
    showNowPlaying(session as unknown as MediaSession, true, 'playback', 'recorder.local');
    const { init } = session.metadata as { init: { artwork: { src: string; sizes: string; type: string }[] } };
    expect(init.artwork.map((image) => [image.sizes, image.type])).toEqual([
      ['192x192', 'image/png'],
      ['512x512', 'image/png'],
    ]);
    expect(init).toMatchObject(describeNowPlaying('playback', 'recorder.local'));
  });
});

describe('bindSessionActions, unwiring', () => {
  it('clears stop as well as play and pause', () => {
    const session = fakeSession();
    const unbind = bindSessionActions(session as unknown as MediaSession, { play: vi.fn(), pause: vi.fn() });
    unbind();
    expect([...session.handlers.entries()]).toEqual([
      ['play', null],
      ['pause', null],
      ['stop', null],
    ]);
  });

  it('carries on unwiring when the browser throws for an action it never bound', () => {
    const session = fakeSession();
    const unbind = bindSessionActions(session as unknown as MediaSession, { play: vi.fn(), pause: vi.fn() });
    session.setActionHandler = (action: string, handler: (() => void) | null) => {
      if (action === 'pause') {
        throw new Error('not supported');
      }
      session.handlers.set(action, handler);
    };
    expect(() => unbind()).not.toThrow();
    expect(session.handlers.get('play')).toBeNull();
    expect(session.handlers.get('stop')).toBeNull();
  });

  it('returns a harmless unbind without the API', () => {
    const unbind = bindSessionActions(undefined, { play: vi.fn(), pause: vi.fn() });
    expect(() => unbind()).not.toThrow();
  });
});
