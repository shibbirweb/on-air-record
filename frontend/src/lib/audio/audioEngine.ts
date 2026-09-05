/**
 * Web Audio playback scheduler.
 *
 * A stream of PCM frames cannot be handed to an `<audio>` element or to `decodeAudioData`, so playback is
 * scheduled by hand. The core idea is a scheduling clock that is deliberately not `currentTime`: it starts
 * one jitter buffer ahead of the present and then advances by exactly the duration of every buffer that is
 * queued. Because each buffer starts precisely where the previous one ended, playback is sample continuous
 * with no clicks between frames.
 *
 * The engine lives outside React entirely. Rebuilding an audio graph during a render causes audible
 * artefacts, and audio arrives far too often to be React state, so a component owns an instance through a
 * ref and reads from it on an animation frame.
 */

import type { AudioFrame } from '@/api/types';

/** How far ahead of the present the first buffer is scheduled. */
const DEFAULT_JITTER_SECONDS = 0.15;

/**
 * Scheduling further ahead than this means latency has crept up, usually after a backgrounded tab, so the
 * clock is pulled back rather than allowed to keep drifting.
 */
const MAX_LEAD_MULTIPLIER = 6;

export type AudioEngineStats = {
  running: boolean;
  bufferedSeconds: number;
  scheduledFrames: number;
  droppedFrames: number;
  resyncs: number;
  sampleRate: number;
};

export class AudioEngine {
  private context: AudioContext | null = null;
  private gainNode: GainNode | null = null;
  private analyserNode: AnalyserNode | null = null;

  /** Context time the next buffer will start at. */
  private nextStartTime = 0;
  private jitterSeconds = DEFAULT_JITTER_SECONDS;
  private volume = 1;
  private muted = false;

  /** Anchor pairing a context time with the media timestamp playing at that moment. */
  private anchorContextTime = 0;
  private anchorMediaMs = 0;

  private scheduledFrames = 0;
  private droppedFrames = 0;
  private resyncs = 0;
  private sources = new Set<AudioBufferSourceNode>();

  /**
   * Create the audio graph. Must be called from a user gesture, because browsers refuse to start an
   * `AudioContext` any other way and a context created outside one stays suspended forever.
   */
  async start(): Promise<void> {
    if (!this.context) {
      const context = new AudioContext({ latencyHint: 'interactive' });
      const gainNode = context.createGain();
      const analyserNode = context.createAnalyser();

      // 2048 samples is about 43 ms at 48 kHz: long enough for a stable waveform and short enough that
      // the drawing still looks live.
      analyserNode.fftSize = 2048;
      analyserNode.smoothingTimeConstant = 0.6;

      gainNode.gain.value = this.muted ? 0 : this.volume;
      gainNode.connect(analyserNode);
      analyserNode.connect(context.destination);

      this.context = context;
      this.gainNode = gainNode;
      this.analyserNode = analyserNode;
    }

    if (this.context.state === 'suspended') {
      await this.context.resume();
    }

    this.resetClock();
  }

  /** Stop playing and release the scheduled buffers, keeping the graph for a quick restart. */
  async suspend(): Promise<void> {
    this.flush();
    if (this.context && this.context.state === 'running') {
      await this.context.suspend();
    }
  }

  /** Tear the graph down completely. */
  async close(): Promise<void> {
    this.flush();
    const context = this.context;
    this.context = null;
    this.gainNode = null;
    this.analyserNode = null;
    if (context) {
      await context.close().catch(() => undefined);
    }
  }

  get running(): boolean {
    return this.context?.state === 'running';
  }

  get analyser(): AnalyserNode | null {
    return this.analyserNode;
  }

  setVolume(volume: number): void {
    this.volume = Math.min(Math.max(volume, 0), 1);
    this.applyGain();
  }

  setMuted(muted: boolean): void {
    this.muted = muted;
    this.applyGain();
  }

  setJitterSeconds(seconds: number): void {
    this.jitterSeconds = Math.min(Math.max(seconds, 0.03), 2);
  }

  /**
   * Drop everything queued and restart the clock.
   *
   * Called on a seek: the frames already scheduled belong to the old position, and playing them out first
   * would mean up to a buffer of the wrong audio after every scrub.
   */
  flush(): void {
    for (const source of this.sources) {
      try {
        source.onended = null;
        source.stop();
      } catch {
        // Already finished. Nothing to stop.
      }
    }
    this.sources.clear();
    this.resetClock();
  }

  /** Queue one decoded frame for playback. */
  enqueue(frame: AudioFrame): void {
    const context = this.context;
    const gainNode = this.gainNode;
    if (!context || !gainNode || context.state !== 'running') {
      return;
    }

    const channels = Math.max(frame.channels, 1);
    const framesPerChannel = Math.floor(frame.samples.length / channels);
    if (framesPerChannel === 0) {
      return;
    }

    // The buffer keeps the source sample rate. Web Audio resamples it into the context rate on playback,
    // which is why the server never has to resample and a 44.1 kHz interface just works.
    const buffer = context.createBuffer(channels, framesPerChannel, frame.sampleRate);
    for (let channel = 0; channel < channels; channel += 1) {
      const target = buffer.getChannelData(channel);
      for (let index = 0; index < framesPerChannel; index += 1) {
        target[index] = frame.samples[index * channels + channel];
      }
    }

    const now = context.currentTime;
    const maxLead = this.jitterSeconds * MAX_LEAD_MULTIPLIER;

    if (this.nextStartTime < now) {
      // The queue ran dry, usually a network stall or a backgrounded tab. Restarting the clock costs one
      // audible gap, which is far better than accumulating latency that never recovers.
      this.nextStartTime = now + this.jitterSeconds;
      this.resyncs += 1;
    } else if (this.nextStartTime - now > maxLead) {
      // Latency crept up. Pull the clock back to the target lead and skip what is between.
      this.nextStartTime = now + this.jitterSeconds;
      this.resyncs += 1;
    }

    const source = context.createBufferSource();
    source.buffer = buffer;
    source.connect(gainNode);
    source.start(this.nextStartTime);

    this.sources.add(source);
    source.onended = () => {
      this.sources.delete(source);
    };

    // Remember which media moment is playing at which context time, so the playhead can be read later
    // without polling the audio thread.
    this.anchorContextTime = this.nextStartTime;
    this.anchorMediaMs = frame.timestampMs;

    this.nextStartTime += buffer.duration;
    this.scheduledFrames += 1;
  }

  /**
   * Media timestamp currently coming out of the speakers, or `null` before anything has played.
   *
   * Derived from the audio clock rather than from the newest frame received, because the newest frame is
   * a jitter buffer ahead of what the listener is actually hearing and a playhead that runs ahead of the
   * sound looks broken.
   */
  currentPlayheadMs(): number | null {
    const context = this.context;
    if (!context || this.anchorContextTime === 0) {
      return null;
    }

    const elapsedSeconds = context.currentTime - this.anchorContextTime;
    return this.anchorMediaMs + elapsedSeconds * 1000;
  }

  stats(): AudioEngineStats {
    const context = this.context;
    const bufferedSeconds = context ? Math.max(this.nextStartTime - context.currentTime, 0) : 0;

    return {
      running: this.running,
      bufferedSeconds,
      scheduledFrames: this.scheduledFrames,
      droppedFrames: this.droppedFrames,
      resyncs: this.resyncs,
      sampleRate: context?.sampleRate ?? 0,
    };
  }

  private applyGain(): void {
    if (!this.gainNode || !this.context) {
      return;
    }
    // A short ramp instead of a step, because an instant gain change is an audible click.
    const target = this.muted ? 0 : this.volume;
    this.gainNode.gain.setTargetAtTime(target, this.context.currentTime, 0.015);
  }

  private resetClock(): void {
    const context = this.context;
    this.nextStartTime = context ? context.currentTime + this.jitterSeconds : 0;
    this.anchorContextTime = 0;
    this.anchorMediaMs = 0;
  }
}
