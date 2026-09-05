import { useEffect, useRef } from 'react';

/**
 * Call `frame` once per animation frame while `active` is true.
 *
 * Used for everything that redraws continuously: the waveform, the meter, the playhead. These read from
 * refs and draw straight to a canvas rather than going through React state, because sixty state updates a
 * second would re render the whole panel for no visual benefit.
 */
export function useAnimationFrame(frame: (timestamp: number) => void, active = true): void {
  const frameRef = useRef(frame);

  // Refreshed after every render, so the loop always draws with the newest closure and never has to be
  // torn down and restarted when a dependency changes.
  useEffect(() => {
    frameRef.current = frame;
  });

  useEffect(() => {
    if (!active) {
      return;
    }

    let handle = 0;
    const tick = (timestamp: number) => {
      frameRef.current(timestamp);
      handle = window.requestAnimationFrame(tick);
    };

    handle = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(handle);
  }, [active]);
}
