/**
 * Oscilloscope of what is actually coming out of the speakers.
 *
 * Reads the `AnalyserNode` on the output of the audio graph rather than the frames arriving on the socket,
 * so it shows what the listener is hearing right now, jitter buffer and all. A visualiser fed from the
 * network would run a buffer ahead of the sound, which is subtly wrong in a way people notice.
 */

import { useCallback, useEffect, useRef } from 'react';

import { useAnimationFrame } from '@/hooks/useAnimationFrame';
import type { AudioEngine } from '@/lib/audio/audioEngine';
import { cn } from '@/lib/utils';

type LiveWaveformProps = {
  engine: AudioEngine;
  className?: string;
  height?: number;
};

export function LiveWaveform({ engine, className, height = 96 }: LiveWaveformProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const bufferRef = useRef<Float32Array<ArrayBuffer> | null>(null);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) {
      return;
    }

    const ratio = window.devicePixelRatio || 1;
    const width = container.clientWidth;
    if (width === 0) {
      return;
    }

    if (canvas.width !== Math.floor(width * ratio) || canvas.height !== Math.floor(height * ratio)) {
      canvas.width = Math.floor(width * ratio);
      canvas.height = Math.floor(height * ratio);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
    }

    const context = canvas.getContext('2d');
    if (!context) {
      return;
    }

    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, height);

    const styles = getComputedStyle(container);
    const waveColour = styles.getPropertyValue('--wave').trim() || '#f0a';
    const idleColour = styles.getPropertyValue('--wave-muted').trim() || '#666';
    const centreY = height / 2;

    const analyser = engine.analyser;
    if (!analyser) {
      // Nothing is playing yet. A flat line reads as "connected but silent", which is honest.
      context.strokeStyle = idleColour;
      context.lineWidth = 1.5;
      context.beginPath();
      context.moveTo(0, centreY);
      context.lineTo(width, centreY);
      context.stroke();
      return;
    }

    if (!bufferRef.current || bufferRef.current.length !== analyser.fftSize) {
      bufferRef.current = new Float32Array(analyser.fftSize);
    }
    const samples = bufferRef.current;
    analyser.getFloatTimeDomainData(samples);

    context.strokeStyle = waveColour;
    context.lineWidth = 1.5;
    context.lineJoin = 'round';
    context.beginPath();

    // One vertical slice per pixel, taking the extremes inside the slice so a transient between two
    // sample points is still drawn rather than skipped.
    const samplesPerPixel = Math.max(Math.floor(samples.length / width), 1);
    for (let x = 0; x < width; x += 1) {
      const start = x * samplesPerPixel;
      let minimum = 1;
      let maximum = -1;

      for (let offset = 0; offset < samplesPerPixel; offset += 1) {
        const sample = samples[start + offset] ?? 0;
        if (sample < minimum) {
          minimum = sample;
        }
        if (sample > maximum) {
          maximum = sample;
        }
      }

      if (minimum > maximum) {
        minimum = 0;
        maximum = 0;
      }

      context.moveTo(x + 0.5, centreY - maximum * centreY * 0.92);
      context.lineTo(x + 0.5, centreY - minimum * centreY * 0.92);
    }

    context.stroke();
  }, [engine, height]);

  useAnimationFrame(draw);

  useEffect(() => {
    const observer = new ResizeObserver(() => draw());
    if (containerRef.current) {
      observer.observe(containerRef.current);
    }
    return () => observer.disconnect();
  }, [draw]);

  return (
    <div ref={containerRef} className={cn('w-full', className)}>
      <canvas ref={canvasRef} className="w-full" style={{ height }} />
    </div>
  );
}
