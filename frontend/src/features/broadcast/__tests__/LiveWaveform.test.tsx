// @vitest-environment jsdom

/**
 * The oscilloscope above the transport controls. Before anything plays it draws a flat line; once the audio
 * graph has an analyser it traces what is coming out of the speakers, one vertical stroke per pixel from
 * the lowest to the highest sample in that slice, so a transient between two sample points still shows.
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

    const y = (sample: number) => 48 - sample * 48 * 0.92;
    const expected = [
      [0.5, -0.5],
      [1, 0],
      [0, 0],
      [0.25, -1],
    ].flatMap(([highest, lowest], x) => [
      { x: x + 0.5, y: y(highest) },
      { x: x + 0.5, y: y(lowest) },
    ]);
    expect(trace.points).toHaveLength(expected.length);
    trace.points.forEach((point, index) => {
      expect(point.x).toBe(expected[index].x);
      expect(point.y).toBeCloseTo(expected[index].y);
    });
  });

  it('draws flat where the analyser has fewer samples than there are pixels', () => {
    engine.analyser = analyserWith([1, -1]);
    renderWaveform();
    drawFrame();

    const [, , , , thirdTop, thirdBottom, fourthTop, fourthBottom] = drawing.paths[0].points;
    for (const point of [thirdTop, thirdBottom, fourthTop, fourthBottom]) {
      expect(point.y).toBe(48);
    }
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
