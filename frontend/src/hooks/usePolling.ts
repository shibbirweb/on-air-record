import { useEffect, useRef } from 'react';

/**
 * Run `task` immediately and then on an interval.
 *
 * Polling pauses while the tab is hidden. A control room page is often left open for days, and a
 * background tab that keeps fetching burns battery for data nobody is looking at.
 */
export function usePolling(task: () => void | Promise<void>, intervalMs: number): void {
  const taskRef = useRef(task);

  // Refreshed after every render so the interval always calls the newest closure without restarting.
  useEffect(() => {
    taskRef.current = task;
  });

  useEffect(() => {
    let timer: number | null = null;

    const run = () => {
      void taskRef.current();
    };

    const start = () => {
      if (timer === null) {
        run();
        timer = window.setInterval(run, intervalMs);
      }
    };

    const stop = () => {
      if (timer !== null) {
        window.clearInterval(timer);
        timer = null;
      }
    };

    const onVisibilityChange = () => {
      if (document.hidden) {
        stop();
      } else {
        start();
      }
    };

    if (!document.hidden) {
      start();
    }
    document.addEventListener('visibilitychange', onVisibilityChange);

    return () => {
      stop();
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [intervalMs]);
}
