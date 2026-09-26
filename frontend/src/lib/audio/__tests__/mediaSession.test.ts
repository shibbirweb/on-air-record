import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { bindSessionActions, describeNowPlaying, showNowPlaying } from '../mediaSession';

/** Just enough of `MediaSession` to see what the module does with it. */
function fakeSession() {
  const handlers = new Map<string, (() => void) | null>();
  return {
    metadata: null as unknown,
    playbackState: 'none' as MediaSessionPlaybackState,
    handlers,
    setActionHandler(action: string, handler: (() => void) | null) {
      handlers.set(action, handler);
    },
  };
}

describe('describeNowPlaying', () => {
  it('names live and history, and the recorder in the album line', () => {
    expect(describeNowPlaying('live', 'recorder.local')).toEqual({
      title: 'Live',
      artist: 'On Air Record',
      album: 'recorder.local',
    });
    expect(describeNowPlaying('playback', '10.0.0.5').title).toBe('Listening back');
  });

  it('keeps saying live while paused on the live feed', () => {
    expect(describeNowPlaying('paused', 'recorder.local').title).toBe('Live');
  });
});

describe('showNowPlaying', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'MediaMetadata',
      class {
        init: unknown;
        constructor(init: unknown) {
          this.init = init;
        }
      },
    );
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('sets what is playing and whether it is', () => {
    const session = fakeSession();
    showNowPlaying(session as unknown as MediaSession, true, 'live', 'recorder.local');
    expect(session.playbackState).toBe('playing');
    expect((session.metadata as { init: { title: string; artwork: unknown[] } }).init.title).toBe('Live');
    expect((session.metadata as { init: { artwork: unknown[] } }).init.artwork).toHaveLength(2);

    showNowPlaying(session as unknown as MediaSession, false, 'playback', 'recorder.local');
    expect(session.playbackState).toBe('paused');
  });

  it('does nothing without the API', () => {
    expect(() => showNowPlaying(undefined, true, 'live', 'recorder.local')).not.toThrow();
  });

  it('still reports the state where MediaMetadata is missing', () => {
    vi.unstubAllGlobals();
    const session = fakeSession();
    showNowPlaying(session as unknown as MediaSession, true, 'live', 'recorder.local');
    expect(session.metadata).toBeNull();
    expect(session.playbackState).toBe('playing');
  });
});

describe('bindSessionActions', () => {
  it('wires play, pause and stop, and unwires them', () => {
    const session = fakeSession();
    const play = vi.fn();
    const pause = vi.fn();
    const unbind = bindSessionActions(session as unknown as MediaSession, { play, pause });

    session.handlers.get('play')?.();
    session.handlers.get('pause')?.();
    session.handlers.get('stop')?.();
    expect(play).toHaveBeenCalledTimes(1);
    expect(pause).toHaveBeenCalledTimes(2);

    unbind();
    expect(session.handlers.get('play')).toBeNull();
    expect(session.handlers.get('pause')).toBeNull();
  });

  it('carries on when the browser rejects an action', () => {
    const session = fakeSession();
    session.setActionHandler = (action: string, handler: (() => void) | null) => {
      if (action === 'stop') {
        throw new Error('not supported');
      }
      session.handlers.set(action, handler);
    };
    expect(() =>
      bindSessionActions(session as unknown as MediaSession, { play: vi.fn(), pause: vi.fn() }),
    ).not.toThrow();
    expect(session.handlers.has('play')).toBe(true);
  });

  it('does nothing without the API', () => {
    expect(bindSessionActions(undefined, { play: vi.fn(), pause: vi.fn() })).toBeTypeOf('function');
  });
});
