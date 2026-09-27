/**
 * The connection store: whether the audio socket is up, what the server said about the stream, and the
 * input meter it feeds.
 *
 * The one rule with any logic is that a dropped socket zeroes the meter rather than freezing it at the
 * last reading, which would look like live input on a dead connection.
 */

import { beforeEach, describe, expect, it } from 'vitest';

import type { StreamInfoMessage } from '@/api/types';

import { useConnectionStore } from '../useConnectionStore';

const INFO: StreamInfoMessage = {
  type: 'stream-info',
  sampleRate: 48_000,
  channels: 1,
  frameMs: 100,
  mode: 'live',
  serverTimeMs: 1_757_034_000_000,
  liveEdgeMs: 1_757_034_000_000,
  earliestMs: null,
  capturing: true,
};

beforeEach(() => {
  useConnectionStore.setState({
    connected: false,
    streamInfo: null,
    levels: { rms: 0, peak: 0 },
    lastError: null,
  });
});

describe('useConnectionStore', () => {
  it('starts disconnected, silent and with nothing known about the stream', () => {
    const state = useConnectionStore.getState();
    expect(state.connected).toBe(false);
    expect(state.streamInfo).toBeNull();
    expect(state.levels).toEqual({ rms: 0, peak: 0 });
    expect(state.lastError).toBeNull();
  });

  it('keeps the meter reading while the socket stays up', () => {
    useConnectionStore.getState().setConnected(true);
    useConnectionStore.getState().setLevels(0.2, 0.7);
    useConnectionStore.getState().setConnected(true);
    expect(useConnectionStore.getState().levels).toEqual({ rms: 0.2, peak: 0.7 });
  });

  it('drops the meter to silence when the socket goes down', () => {
    useConnectionStore.getState().setConnected(true);
    useConnectionStore.getState().setLevels(0.2, 0.7);
    useConnectionStore.getState().setConnected(false);

    expect(useConnectionStore.getState().connected).toBe(false);
    expect(useConnectionStore.getState().levels).toEqual({ rms: 0, peak: 0 });
  });

  it('records what the server reported about the stream', () => {
    useConnectionStore.getState().setStreamInfo(INFO);
    expect(useConnectionStore.getState().streamInfo).toEqual(INFO);
  });

  it('replaces the meter reading as a pair', () => {
    useConnectionStore.getState().setLevels(0.1, 0.3);
    useConnectionStore.getState().setLevels(0.05, 0.1);
    expect(useConnectionStore.getState().levels).toEqual({ rms: 0.05, peak: 0.1 });
  });

  it('sets and clears the last error', () => {
    useConnectionStore.getState().setError('device unplugged');
    expect(useConnectionStore.getState().lastError).toBe('device unplugged');
    useConnectionStore.getState().setError(null);
    expect(useConnectionStore.getState().lastError).toBeNull();
  });
});
