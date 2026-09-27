// @vitest-environment jsdom
/**
 * `usePolling`: run a task at once and then on an interval, pausing while the tab is hidden.
 *
 * Time is faked and `document.hidden` is driven by the test, so the schedule can be asserted exactly:
 * when the task runs, that hiding the tab stops it, that showing the tab runs it at once and resumes, that
 * a new closure is used without restarting the timer, and that unmounting leaves nothing running.
 */

import '@/test/dom';

import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { usePolling } from '../usePolling';

let hidden = false;

function setHidden(next: boolean) {
  hidden = next;
  document.dispatchEvent(new Event('visibilitychange'));
}

beforeEach(() => {
  vi.useFakeTimers();
  hidden = false;
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => hidden });
});

afterEach(() => {
  vi.useRealTimers();
  Reflect.deleteProperty(document, 'hidden');
});

describe('usePolling', () => {
  it('runs the task at once and then every interval', () => {
    const task = vi.fn();
    renderHook(() => usePolling(task, 1_000));
    expect(task).toHaveBeenCalledTimes(1);

    vi.advanceTimersByTime(999);
    expect(task).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1);
    expect(task).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(3_000);
    expect(task).toHaveBeenCalledTimes(5);
  });

  it('does not start while the tab is hidden', () => {
    hidden = true;
    const task = vi.fn();
    renderHook(() => usePolling(task, 1_000));
    vi.advanceTimersByTime(10_000);
    expect(task).not.toHaveBeenCalled();
  });

  it('stops while the tab is hidden, and runs at once when it is shown again', () => {
    const task = vi.fn();
    renderHook(() => usePolling(task, 1_000));
    setHidden(true);
    vi.advanceTimersByTime(10_000);
    expect(task).toHaveBeenCalledTimes(1);

    setHidden(false);
    expect(task).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(1_000);
    expect(task).toHaveBeenCalledTimes(3);
  });

  it('does not run twice or double the timer when shown while already running', () => {
    const task = vi.fn();
    renderHook(() => usePolling(task, 1_000));
    setHidden(false);
    expect(task).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1_000);
    expect(task).toHaveBeenCalledTimes(2);
  });

  it('calls the newest task without restarting the interval', () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(({ task }) => usePolling(task, 1_000), {
      initialProps: { task: first },
    });
    vi.advanceTimersByTime(500);
    rerender({ task: second });
    expect(second).not.toHaveBeenCalled();

    vi.advanceTimersByTime(500);
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
  });

  it('restarts on the new schedule when the interval changes', () => {
    const task = vi.fn();
    const { rerender } = renderHook(({ interval }) => usePolling(task, interval), {
      initialProps: { interval: 1_000 },
    });
    rerender({ interval: 5_000 });
    // Restarting runs the task at once, as on mount.
    expect(task).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(4_999);
    expect(task).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(1);
    expect(task).toHaveBeenCalledTimes(3);
  });

  it('leaves nothing running after unmount, not even the visibility listener', () => {
    const task = vi.fn();
    const { unmount } = renderHook(() => usePolling(task, 1_000));
    unmount();
    vi.advanceTimersByTime(10_000);
    setHidden(true);
    setHidden(false);
    expect(task).toHaveBeenCalledTimes(1);
  });

  it('does not wait for an async task before scheduling the next run', () => {
    const task = vi.fn(() => new Promise<void>(() => undefined));
    renderHook(() => usePolling(task, 1_000));
    vi.advanceTimersByTime(2_000);
    expect(task).toHaveBeenCalledTimes(3);
  });
});
