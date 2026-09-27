// @vitest-environment jsdom
/**
 * `useAnimationFrame`: call a draw function once per animation frame while active.
 *
 * The frame queue is replaced by one the test flushes by hand, so it can assert that each frame draws
 * once and queues the next, that the newest closure draws without the loop restarting, and that turning
 * it off or unmounting cancels the frame that was queued rather than leaving a loop running.
 */

import '@/test/dom';

import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAnimationFrame } from '../useAnimationFrame';

let queued = new Map<number, FrameRequestCallback>();
let nextHandle = 1;
let cancelled: number[] = [];

/** Run every frame queued so far, as the browser would on its next paint. */
function paint(timestamp: number) {
  const running = [...queued.values()];
  queued = new Map();
  for (const callback of running) {
    callback(timestamp);
  }
}

beforeEach(() => {
  queued = new Map();
  nextHandle = 1;
  cancelled = [];
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
    const handle = nextHandle;
    nextHandle += 1;
    queued.set(handle, callback);
    return handle;
  });
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation((handle) => {
    cancelled.push(handle);
    queued.delete(handle);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('useAnimationFrame', () => {
  it('draws once per frame with the frame timestamp, queueing the next each time', () => {
    const frame = vi.fn();
    renderHook(() => useAnimationFrame(frame));
    expect(frame).not.toHaveBeenCalled();

    paint(16);
    paint(32);
    expect(frame.mock.calls).toEqual([[16], [32]]);
    expect(queued.size).toBe(1);
  });

  it('does nothing while inactive', () => {
    const frame = vi.fn();
    renderHook(() => useAnimationFrame(frame, false));
    paint(16);
    expect(frame).not.toHaveBeenCalled();
    expect(queued.size).toBe(0);
  });

  it('draws with the newest closure without restarting the loop', () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(({ frame }) => useAnimationFrame(frame), {
      initialProps: { frame: first },
    });
    paint(16);
    rerender({ frame: second });
    paint(32);

    expect(first.mock.calls).toEqual([[16]]);
    expect(second.mock.calls).toEqual([[32]]);
    expect(cancelled).toEqual([]);
  });

  it('cancels the queued frame when turned off, and starts again when turned on', () => {
    const frame = vi.fn();
    const { rerender } = renderHook(({ active }) => useAnimationFrame(frame, active), {
      initialProps: { active: true },
    });
    paint(16);
    rerender({ active: false });
    expect(cancelled).toHaveLength(1);
    paint(32);
    expect(frame).toHaveBeenCalledTimes(1);

    rerender({ active: true });
    paint(48);
    expect(frame).toHaveBeenCalledTimes(2);
  });

  it('cancels the frame queued most recently on unmount, leaving no loop behind', () => {
    const frame = vi.fn();
    const { unmount } = renderHook(() => useAnimationFrame(frame));
    paint(16);
    paint(32);
    const pending = [...queued.keys()];
    unmount();

    expect(cancelled).toEqual(pending);
    paint(48);
    expect(frame).toHaveBeenCalledTimes(2);
  });
});
