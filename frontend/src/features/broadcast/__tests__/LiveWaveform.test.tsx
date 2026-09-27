// @vitest-environment jsdom

/**
 * The oscilloscope above the transport controls. Before anything plays it draws a flat line; once the audio
 * graph has an analyser it traces what is coming out of the speakers, one vertical stroke per pixel from
 * the lowest to the highest sample in that slice, so a transient between two sample points still shows.
 * The trace is magnified to the signal (lib/audio/waveformScale.ts, tested there), and every slice is at
 * least a hairline, so quiet or silent audio still draws a line.
 * It reads the analyser every frame, straight off the engine, so none of this goes through React.
 *
 * Drawn onto a recording stand in for the canvas, since jsdom has none. jsdom resolves no stylesheet, so
 * the colours are the fallbacks: the trace '#f0a' and the idle line '#666'.
 */

import '@/test/dom';

import { render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { recordDrawing } from '@/features/timeline/__tests__/drawing';
import type { DrawingRecorder } from '@/features/timeline/__tests__/drawing';
import type { AudioEngine } from '@/lib/audio/audioEngine';

import { LiveWaveform } from '../LiveWaveform';

const WAVE = '#f0a';
const IDLE = '#666';
const WIDTH = 4;

/** Just the part of the engine the waveform reads. */
type FakeEngine = { analyser: AnalyserNode | null };

function analyserWith(samples: number[]): AnalyserNode {
  return {
    fftSize: samples.length,
    getFloatTimeDomainData: (target: Float32Array) => {
      target.set(samples);
    },
  } as unknown as AnalyserNode;
}

let drawing: DrawingRecorder;
let engine: FakeEngine;

function renderWaveform(height?: number) {
  const view = render(<LiveWaveform engine={engine as unknown as AudioEngine} height={height} />);
  const canvas = view.container.querySelector('canvas') as HTMLCanvasElement;
  return { ...view, canvas };
}

function drawFrame() {
  drawing.clear();
  drawing.frame();
}

beforeEach(() => {
  drawing = recordDrawing(WIDTH);
  engine = { analyser: null };
});

afterEach(() => {
  drawing.restore();
});

describe('before anything plays', () => {
  it('draws a flat line across the middle, which reads as connected but silent', () => {
    renderWaveform();
    drawFrame();
    expect(drawing.paths).toEqual([
      {
        kind: 'stroke',
        points: [
          { x: 0, y: 48 },
          { x: WIDTH, y: 48 },
        ],
        style: IDLE,
        lineWidth: 1.5,
        alpha: 1,
        dashed: false,
      },
    ]);
  });
});

describe('while audio plays', () => {
  it('draws one stroke per pixel from the highest to the lowest sample in its slice', () => {
    // Two samples a pixel.
    engine.analyser = analyserWith([0.5, -0.5, 1, 0, 0, 0, -1, 0.25]);
    renderWaveform();
    drawFrame();

    expect(drawing.paths).toHaveLength(1);
    const trace = drawing.paths[0];
    expect(trace).toMatchObject({ kind: 'stroke', style: WAVE, lineWidth: 1.5 });

    // The loudest sample is full scale, so the first frame shrinks the trace to fill 80 percent.
    const y = (sample: number) => 48 - sample * 0.8 * 48 * 0.92;
    const expected = [
      [0.5, -0.5],
      [1, 0],
      [0, 0],
      [0.25, -1],
    ].flatMap(([highest, lowest], x) =>
      highest === lowest
        ? // A silent slice is still drawn, as a hairline around its middle.
          [
            { x: x + 0.5, y: y(highest) - 0.5 },
            { x: x + 0.5, y: y(lowest) + 0.5 },
          ]
        : [
            { x: x + 0.5, y: y(highest) },
            { x: x + 0.5, y: y(lowest) },
          ],
    );
    expect(trace.points).toHaveLength(expected.length);
    trace.points.forEach((point, index) => {
      expect(point.x).toBe(expected[index].x);
      expect(point.y).toBeCloseTo(expected[index].y);
    });
  });

  it('draws a hairline, not nothing, where a slice is silent', () => {
    // A stroke of no length draws nothing, so silence used to leave an empty box.
    engine.analyser = analyserWith([1, -1]);
    renderWaveform();
    drawFrame();

    const [, , , , thirdTop, thirdBottom, fourthTop, fourthBottom] = drawing.paths[0].points;
    expect([thirdTop.y, thirdBottom.y, fourthTop.y, fourthBottom.y]).toEqual([47.5, 48.5, 47.5, 48.5]);
  });

  it('magnifies quiet audio frame by frame until it can be seen', () => {
    // Speech a few percent of full scale is under a pixel tall at true size.
    engine.analyser = analyserWith([0.05, -0.05, 0.05, -0.05, 0.05, -0.05, 0.05, -0.05]);
    renderWaveform();
    const spread = () => {
      drawFrame();
      const [top, bottom] = drawing.paths[0].points;
      return bottom.y - top.y;
    };
    const first = spread();
    for (let frame = 0; frame < 300; frame += 1) {
      spread();
    }
    const settled = spread();
    expect(first).toBeLessThan(10);
    expect(settled).toBeCloseTo(2 * 0.8 * 48 * 0.92, 1);
  });

  it('picks up the analyser the moment the engine has one, without a render', () => {
    renderWaveform();
    drawFrame();
    expect(drawing.paths[0].style).toBe(IDLE);

    engine.analyser = analyserWith([0.5, -0.5, 0.5, -0.5, 0.5, -0.5, 0.5, -0.5]);
    drawFrame();
    expect(drawing.paths[0].style).toBe(WAVE);
  });
});

describe('its size', () => {
  it('fills its container at the default height', () => {
    const { canvas } = renderWaveform();
    drawFrame();
    expect(canvas.style.width).toBe(`${WIDTH}px`);
    expect(canvas.style.height).toBe('96px');
    expect(canvas.width).toBe(WIDTH * (window.devicePixelRatio || 1));
  });

  it('takes a custom height and centres on it', () => {
    const { canvas } = renderWaveform(60);
    drawFrame();
    expect(canvas.style.height).toBe('60px');
    expect(drawing.paths[0].points.map((point) => point.y)).toEqual([30, 30]);
  });
});
