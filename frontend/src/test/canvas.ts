/**
 * A stand in for a canvas and the animation loop, so drawing code can be tested in jsdom, which has no
 * canvas of its own.
 *
 * The fake context records every filled rectangle with the colour and transparency in force when it was
 * drawn, and does nothing for everything else. That is enough to assert what a layer paints and where,
 * which is where drawing code goes wrong; whether it looks right is for the browser test.
 */

import { vi } from 'vitest';

export type Paint = {
  x: number;
  y: number;
  width: number;
  height: number;
  fillStyle: string;
  alpha: number;
};

export type CanvasRecorder = {
  paints: Paint[];
  /** Run the queued animation frames once, which is one draw of every canvas on screen. */
  frame: () => void;
  restore: () => void;
};

/** Record drawing on every canvas, as if each were `width` CSS pixels wide. */
export function recordCanvases(width: number): CanvasRecorder {
  const paints: Paint[] = [];
  const state: Record<string | symbol, unknown> = { fillStyle: '#000', globalAlpha: 1 };
  const context = new Proxy(state, {
    get(target, property) {
      if (property === 'fillRect') {
        return (x: number, y: number, rectWidth: number, height: number) =>
          paints.push({
            x,
            y,
            width: rectWidth,
            height,
            fillStyle: String(target.fillStyle),
            alpha: Number(target.globalAlpha),
          });
      }
      if (property === 'measureText') {
        return (text: string) => ({ width: text.length * 6 });
      }
      if (property in target) {
        return target[property];
      }
      return () => undefined;
    },
    set(target, property, value) {
      target[property] = value;
      return true;
    },
  });

  const getContext = vi
    .spyOn(HTMLCanvasElement.prototype, 'getContext')
    .mockImplementation(() => context as unknown as CanvasRenderingContext2D);
  const clientWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth');
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => width });

  let queued: FrameRequestCallback[] = [];
  const requestFrame = vi
    .spyOn(window, 'requestAnimationFrame')
    .mockImplementation((callback) => {
      queued.push(callback);
      return queued.length;
    });
  const cancelFrame = vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined);

  return {
    paints,
    frame: () => {
      const running = queued;
      queued = [];
      for (const callback of running) {
        callback(performance.now());
      }
    },
    restore: () => {
      getContext.mockRestore();
      requestFrame.mockRestore();
      cancelFrame.mockRestore();
      if (clientWidth) {
        Object.defineProperty(HTMLElement.prototype, 'clientWidth', clientWidth);
      }
    },
  };
}
