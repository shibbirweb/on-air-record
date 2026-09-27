// @vitest-environment jsdom
/**
 * `useStreamEngine`, the hook that owns the one socket and the one audio graph and wires them to the
 * stores.
 *
 * The socket, the engine and the lock screen helpers are replaced by recording fakes, because what can go
 * wrong here is the wiring rather than the audio: a server message routed to the wrong store, a transport
 * action that forgets to flush or to tell the server, a player state reported twice or not again after a
 * reconnect, or something left running after unmount. Whether sound actually comes out is for a person
 * listening, as CLAUDE.md says.
 */

import '@/test/dom';

import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ServerMessage } from '@/api/types';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useListenersStore } from '@/store/useListenersStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

const fakes = vi.hoisted(() => {
  class FakeStreamSocket {
    static instances: FakeStreamSocket[] = [];
    handlers: {
      onFrame: (frame: unknown) => void;
      onMessage: (message: unknown) => void;
      onOpen: () => void;
      onClose: () => void;
    };
    connected = false;
    sent: unknown[] = [];
    connectCalls = 0;
    closeCalls = 0;

    constructor(handlers: FakeStreamSocket['handlers']) {
      this.handlers = handlers;
      FakeStreamSocket.instances.push(this);
    }

    connect() {
      this.connectCalls += 1;
    }

    send(message: unknown) {
      this.sent.push(message);
    }

    close() {
      this.closeCalls += 1;
      this.connected = false;
    }

    /** The server accepted the connection. */
    open() {
      this.connected = true;
      this.handlers.onOpen();
    }

    drop() {
      this.connected = false;
      this.handlers.onClose();
    }
  }

  class FakeAudioEngine {
    static instances: FakeAudioEngine[] = [];
    calls: unknown[][] = [];
    interrupted: (() => void) | null = null;
    playhead: number | null = 1_757_034_000_000;

    constructor() {
      FakeAudioEngine.instances.push(this);
    }

    private record(...call: unknown[]) {
      this.calls.push(call);
    }

    enqueue(frame: unknown) {
      this.record('enqueue', frame);
    }
    flush() {
      this.record('flush');
    }
    setSpeed(speed: number) {
      this.record('setSpeed', speed);
    }
    setVolume(volume: number) {
      this.record('setVolume', volume);
    }
    setMuted(muted: boolean) {
      this.record('setMuted', muted);
    }
    async start() {
      this.record('start');
    }
    async suspend() {
      this.record('suspend');
    }
    async close() {
      this.record('close');
    }
    onInterrupted(handler: (() => void) | null) {
      this.interrupted = handler;
      this.record('onInterrupted', handler === null ? null : 'handler');
    }
    currentPlayheadMs() {
      return this.playhead;
    }
    names() {
      return this.calls.map(([name]) => name);
    }
  }

  return {
    FakeStreamSocket,
    FakeAudioEngine,
    unbind: { calls: 0 },
    bound: [] as { play: () => void; pause: () => void }[],
    shown: [] as [boolean, string, string][],
  };
});

vi.mock('@/api/streamSocket', () => ({ StreamSocket: fakes.FakeStreamSocket }));
vi.mock('@/lib/audio/audioEngine', () => ({ AudioEngine: fakes.FakeAudioEngine }));
vi.mock('@/lib/audio/mediaSession', () => ({
  bindSessionActions: (_session: unknown, actions: { play: () => void; pause: () => void }) => {
    fakes.bound.push(actions);
    return () => {
      fakes.unbind.calls += 1;
    };
  },
  showNowPlaying: (_session: unknown, playing: boolean, mode: string, host: string) => {
    fakes.shown.push([playing, mode, host]);
  },
}));

const { useStreamEngine } = await import('../useStreamEngine');

function socket() {
  const latest = fakes.FakeStreamSocket.instances.at(-1);
  if (!latest) {
    throw new Error('no socket');
  }
  return latest;
}

function engine() {
  const latest = fakes.FakeAudioEngine.instances.at(-1);
  if (!latest) {
    throw new Error('no engine');
  }
  return latest;
}

function message(value: ServerMessage) {
  act(() => {
    socket().handlers.onMessage(value);
  });
}

function controller() {
  const attached = useTransportStore.getState().controller;
  if (!attached) {
    throw new Error('no controller attached');
  }
  return attached;
}

beforeEach(() => {
  fakes.FakeStreamSocket.instances = [];
  fakes.FakeAudioEngine.instances = [];
  fakes.unbind.calls = 0;
  fakes.bound = [];
  fakes.shown = [];
  useTransportStore.setState({
    playing: false,
    mode: 'live',
    followingLive: true,
    requestedPositionMs: null,
    volume: 0.6,
    muted: true,
    speed: 1,
    endOfRecording: false,
    controller: null,
  });
  useConnectionStore.setState({
    connected: true,
    streamInfo: null,
    levels: { rms: 0, peak: 0 },
    lastError: 'stale',
  });
  useListenersStore.setState({ listeners: [] });
  useTimelineStore.setState({ followingLive: true, range: null });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('mounting', () => {
  it('builds one engine, connects one socket and attaches the controller', () => {
    renderHook(() => useStreamEngine());

    expect(fakes.FakeAudioEngine.instances).toHaveLength(1);
    expect(fakes.FakeStreamSocket.instances).toHaveLength(1);
    expect(socket().connectCalls).toBe(1);
    expect(useTransportStore.getState().controller).not.toBeNull();
  });

  it('restores the volume and mute the listener had, and starts disconnected', () => {
    renderHook(() => useStreamEngine());
    expect(engine().calls).toContainEqual(['setVolume', 0.6]);
    expect(engine().calls).toContainEqual(['setMuted', true]);
    expect(useConnectionStore.getState().connected).toBe(false);
  });

  it('keeps the same engine across renders, since rebuilding one clicks and loses the clock', () => {
    const { result, rerender } = renderHook(() => useStreamEngine());
    const first = result.current.engine;
    rerender();
    rerender();
    expect(result.current.engine).toBe(first);
    expect(fakes.FakeAudioEngine.instances).toHaveLength(1);
    expect(fakes.FakeStreamSocket.instances).toHaveLength(1);
  });

  it('reads the playhead off the engine clock', () => {
    const { result } = renderHook(() => useStreamEngine());
    expect(result.current.playheadMs()).toBe(1_757_034_000_000);
    engine().playhead = null;
    expect(result.current.playheadMs()).toBeNull();
  });
});

describe('the socket', () => {
  it('marks the connection up and clears the last error when it opens', () => {
    renderHook(() => useStreamEngine());
    act(() => {
      socket().open();
    });
    expect(useConnectionStore.getState().connected).toBe(true);
    expect(useConnectionStore.getState().lastError).toBeNull();
  });

  it('marks the connection down, drops the listener list and flushes stale audio when it closes', () => {
    renderHook(() => useStreamEngine());
    act(() => {
      socket().open();
      socket().drop();
    });
    expect(useConnectionStore.getState().connected).toBe(false);
    expect(useListenersStore.getState().listeners).toBeNull();
    expect(engine().names()).toContain('flush');
  });

  it('hands every frame to the engine', () => {
    renderHook(() => useStreamEngine());
    const frame = { timestampMs: 1, sampleRate: 48_000, channels: 1, live: true, samples: new Float32Array(1) };
    socket().handlers.onFrame(frame);
    expect(engine().calls).toContainEqual(['enqueue', frame]);
  });
});

describe('server messages', () => {
  it('stores the stream info and takes its mode', () => {
    renderHook(() => useStreamEngine());
    const info = {
      type: 'stream-info' as const,
      sampleRate: 48_000,
      channels: 1,
      frameMs: 100,
      mode: 'playback' as const,
      serverTimeMs: 0,
      liveEdgeMs: null,
      earliestMs: null,
      capturing: true,
    };
    message(info);
    expect(useConnectionStore.getState().streamInfo).toEqual(info);
    expect(useTransportStore.getState().mode).toBe('playback');
  });

  it('takes a mode change', () => {
    renderHook(() => useStreamEngine());
    message({ type: 'mode', mode: 'paused', positionMs: 0 });
    expect(useTransportStore.getState().mode).toBe('paused');
  });

  it('flushes, returns to real time and follows live when the server switches to live', () => {
    renderHook(() => useStreamEngine());
    useTransportStore.setState({ mode: 'playback', followingLive: false, speed: 2 });
    useTimelineStore.setState({ followingLive: false });
    engine().calls = [];

    message({ type: 'switched-to-live', timestampMs: 0 });
    expect(engine().calls.slice(0, 2)).toEqual([['flush'], ['setSpeed', 1]]);
    expect(useTransportStore.getState()).toMatchObject({ mode: 'live', followingLive: true, speed: 1 });
    expect(useTimelineStore.getState().followingLive).toBe(true);
  });

  it('flushes across a gap in the recording', () => {
    renderHook(() => useStreamEngine());
    engine().calls = [];
    message({ type: 'gap', fromMs: 0, toMs: 1 });
    expect(engine().names()).toEqual(['flush']);
  });

  it('records the end of the recording', () => {
    renderHook(() => useStreamEngine());
    message({ type: 'end-of-recording', timestampMs: 0 });
    expect(useTransportStore.getState().endOfRecording).toBe(true);
  });

  it('feeds the meter', () => {
    renderHook(() => useStreamEngine());
    useConnectionStore.setState({ connected: true });
    message({ type: 'level', rms: 0.1, peak: 0.3 });
    expect(useConnectionStore.getState().levels).toEqual({ rms: 0.1, peak: 0.3 });
  });

  it('takes the speed the server applied, in the engine and the store', () => {
    renderHook(() => useStreamEngine());
    message({ type: 'speed', value: 4 });
    expect(engine().calls).toContainEqual(['setSpeed', 4]);
    expect(useTransportStore.getState().speed).toBe(4);
  });

  it('records a server error', () => {
    renderHook(() => useStreamEngine());
    message({ type: 'error', code: 'device', message: 'device unplugged' });
    expect(useConnectionStore.getState().lastError).toBe('device unplugged');
  });

  it('replaces the listener list, and drops it when hidden', () => {
    renderHook(() => useStreamEngine());
    const listeners = [
      {
        id: 1,
        email: null,
        role: null,
        address: '10.0.0.2',
        userAgent: null,
        connectedAtMs: 0,
        activity: 'live' as const,
        fromMs: null,
        player: 'idle' as const,
      },
    ];
    message({ type: 'listeners', listeners });
    expect(useListenersStore.getState().listeners).toEqual(listeners);
    message({ type: 'listeners-hidden' });
    expect(useListenersStore.getState().listeners).toBeNull();
  });

  it('does nothing with a pong', () => {
    renderHook(() => useStreamEngine());
    engine().calls = [];
    const before = useTransportStore.getState();
    message({ type: 'pong', clientTimeMs: 0, serverTimeMs: 0 });
    expect(engine().calls).toEqual([]);
    expect(useTransportStore.getState()).toBe(before);
  });
});

describe('the controller', () => {
  it('starts the engine on play, then applies the current volume and mute', async () => {
    renderHook(() => useStreamEngine());
    useTransportStore.setState({ volume: 0.3, muted: false });
    engine().calls = [];

    await act(async () => {
      await controller().play();
    });
    expect(engine().calls).toEqual([['start'], ['setVolume', 0.3], ['setMuted', false]]);
  });

  it('suspends the engine on pause', () => {
    renderHook(() => useStreamEngine());
    engine().calls = [];
    controller().pause();
    expect(engine().names()).toEqual(['suspend']);
  });

  it('flushes, stops the timeline following live, and asks the server for a whole millisecond on seek', () => {
    renderHook(() => useStreamEngine());
    engine().calls = [];
    act(() => {
      controller().seek(1_757_000_000_000.6);
    });
    expect(engine().names()).toEqual(['flush']);
    expect(useTimelineStore.getState().followingLive).toBe(false);
    expect(socket().sent).toEqual([{ type: 'seek', timestampMs: 1_757_000_000_001 }]);
  });

  it('flushes, follows live and asks the server for the live feed on go live', () => {
    renderHook(() => useStreamEngine());
    useTimelineStore.setState({ followingLive: false });
    engine().calls = [];
    act(() => {
      controller().goLive();
    });
    expect(engine().names()).toEqual(['flush']);
    expect(useTimelineStore.getState().followingLive).toBe(true);
    expect(socket().sent).toEqual([{ type: 'live' }]);
  });

  it('passes volume and mute straight to the engine', () => {
    renderHook(() => useStreamEngine());
    engine().calls = [];
    controller().setVolume(0.9);
    controller().setMuted(false);
    expect(engine().calls).toEqual([
      ['setVolume', 0.9],
      ['setMuted', false],
    ]);
  });

  it('changes speed in both the engine and the server, since either alone only changes the buffer', () => {
    renderHook(() => useStreamEngine());
    engine().calls = [];
    controller().setSpeed(2);
    expect(engine().calls).toEqual([['setSpeed', 2]]);
    expect(socket().sent).toEqual([{ type: 'speed', value: 2 }]);
  });
});

describe('reporting the player state', () => {
  function playerReports() {
    return socket().sent.filter((sent) => (sent as { type: string }).type === 'player');
  }

  it('reports idle once the socket opens, then playing and paused as they happen', () => {
    renderHook(() => useStreamEngine());
    act(() => {
      socket().open();
    });
    act(() => {
      useTransportStore.getState().setPlaying(true);
    });
    act(() => {
      useTransportStore.getState().setPlaying(false);
    });
    expect(playerReports()).toEqual([
      { type: 'player', state: 'idle' },
      { type: 'player', state: 'playing' },
      { type: 'player', state: 'paused' },
    ]);
  });

  it('does not repeat a state for changes that do not affect it', () => {
    renderHook(() => useStreamEngine());
    act(() => {
      socket().open();
      useTransportStore.getState().setVolume(0.1);
      useTransportStore.getState().setMode('playback');
    });
    expect(playerReports()).toEqual([{ type: 'player', state: 'idle' }]);
  });

  it('sends nothing while the socket is down, and the current state again on every reconnect', () => {
    renderHook(() => useStreamEngine());
    act(() => {
      useTransportStore.getState().setPlaying(true);
    });
    expect(playerReports()).toEqual([]);

    act(() => {
      socket().open();
    });
    act(() => {
      socket().drop();
      socket().open();
    });
    expect(playerReports()).toEqual([
      { type: 'player', state: 'playing' },
      { type: 'player', state: 'playing' },
    ]);
  });
});

describe('the lock screen', () => {
  it('binds play and pause to the transport', async () => {
    renderHook(() => useStreamEngine());
    const [actions] = fakes.bound;

    await act(async () => {
      actions.play();
    });
    expect(useTransportStore.getState().playing).toBe(true);

    act(() => {
      actions.pause();
    });
    expect(useTransportStore.getState().playing).toBe(false);
  });

  it('shows what is playing when playing or the mode changes, and not for anything else', () => {
    renderHook(() => useStreamEngine());
    act(() => {
      useTransportStore.getState().setVolume(0.2);
    });
    expect(fakes.shown).toEqual([]);

    act(() => {
      useTransportStore.getState().setPlaying(true);
    });
    act(() => {
      useTransportStore.getState().setMode('playback');
    });
    expect(fakes.shown).toEqual([
      [true, 'live', window.location.host],
      [true, 'playback', window.location.host],
    ]);
  });

  it('pauses the transport when the phone takes the audio away', () => {
    renderHook(() => useStreamEngine());
    useTransportStore.setState({ playing: true });
    act(() => {
      engine().interrupted?.();
    });
    expect(useTransportStore.getState().playing).toBe(false);
  });
});

describe('unmounting', () => {
  it('detaches everything it set up and closes the socket and the engine', () => {
    const { unmount } = renderHook(() => useStreamEngine());
    unmount();

    expect(useTransportStore.getState().controller).toBeNull();
    expect(socket().closeCalls).toBe(1);
    expect(engine().names()).toContain('close');
    expect(engine().interrupted).toBeNull();
    expect(fakes.unbind.calls).toBe(1);
  });

  it('stops watching the transport, so nothing is reported or shown afterwards', () => {
    const { unmount } = renderHook(() => useStreamEngine());
    act(() => {
      socket().open();
    });
    unmount();
    const sentBefore = socket().sent.length;
    socket().connected = true;

    act(() => {
      useTransportStore.getState().setPlaying(true);
    });
    expect(socket().sent).toHaveLength(sentBefore);
    expect(fakes.shown).toEqual([]);
  });
});
