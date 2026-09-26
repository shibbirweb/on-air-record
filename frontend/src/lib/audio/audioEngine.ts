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
 *
 * The graph does not end at the speakers but at a hidden `<audio>` element fed from a media stream. Phones
 * keep a page running with the screen off only while it plays media through an element: pure Web Audio is
 * suspended as soon as the screen locks, which on a phone meant the broadcast stopped with it. Routing
 * through an element makes the page a media player like any radio site, and is what lock screen controls
 * attach to. Where the element cannot play, the graph falls back to the speakers directly, as it always did.
 */

import type { AudioFrame } from '@/api/types';
import { needsNotificationKeeper, silentWav } from '@/lib/audio/silence';

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
  speed: number;
};

/** Safari's Audio Session API, not yet in the DOM typings. */
type AudioSessionNavigator = Navigator & { audioSession?: { type: string } };

export class AudioEngine {
  private context: AudioContext | null = null;
  private gainNode: GainNode | null = null;
  private analyserNode: AnalyserNode | null = null;
  /** The element carrying the graph's output, or null when it plays straight to the speakers. */
  private output: HTMLAudioElement | null = null;
  /** Everywhere but WebKit: a looping silent clip, so the browser shows media controls. See `silence.ts`. */
  private keeper: HTMLAudioElement | null = null;
  /**
   * Whether the listener wants sound. Set by `start` and cleared by `suspend`, so that anything else that
   * stops the audio, the phone locking or another app taking the speaker, is recognised as an interruption
   * to recover from rather than a pause somebody asked for.
   */
  private wantsToPlay = false;
  /** Called when the phone pauses playback on its own, so the transport can stop claiming to play. */
  private interruptedHandler: (() => void) | null = null;
  private readonly onVisibilityChange = () => {
    if (document.visibilityState === 'visible') {
      this.recover();
    }
  };

  /** Context time the next buffer will start at. */
  private nextStartTime = 0;
  private jitterSeconds = DEFAULT_JITTER_SECONDS;
  private volume = 1;
  private muted = false;
  /**
   * Rate each buffer is played back at.
   *
   * The server paces frames to match, so this is what keeps the queue from growing at speeds above one.
   * It shifts pitch along with tempo, the way tape does, because preserving pitch needs a phase vocoder
   * and that is a great deal of machinery for a control whose job is scanning through recordings.
   */
  private speed = 1;

  /** Anchor pairing a context time with the media timestamp playing at that moment. */
  private anchorContextTime = 0;
  private anchorMediaMs = 0;
  /** Rate in force when the anchor was set, so the playhead does not jump when the speed changes. */
  private anchorSpeed = 1;

  private scheduledFrames = 0;
  private droppedFrames = 0;
  private resyncs = 0;
  private sources = new Set<AudioBufferSourceNode>();

  /**
   * Create the audio graph. Must be called from a user gesture, because browsers refuse to start an
   * `AudioContext` any other way and a context created outside one stays suspended forever.
   */
  async start(): Promise<void> {
    this.wantsToPlay = true;
    // Tells an iPhone this is media playback rather than sound effects, so it keeps playing with the
    // screen locked and ignores the silent switch. Only Safari has it, and only recent versions.
    const audioSession = (navigator as AudioSessionNavigator).audioSession;
    if (audioSession) {
      try {
        audioSession.type = 'playback';
      } catch {
        // Refused, or read only in this browser. Playback still works, just not as media.
      }
    }

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

      this.context = context;
      this.gainNode = gainNode;
      this.analyserNode = analyserNode;
      this.output = this.routeOutput(context, analyserNode);
      this.keeper = this.output && needsNotificationKeeper(navigator.userAgent) ? this.createKeeper() : null;
      context.onstatechange = () => this.recover();
      document.addEventListener('visibilitychange', this.onVisibilityChange);
    }

    // Started before any await: phones allow media to begin only while the tap that asked for it is
    // still being handled, and an await hands control back first.
    const playing = this.output?.play();
    const keeping = this.keeper?.play();

    if (this.context.state !== 'running') {
      await this.context.resume();
    }
    if (playing) {
      await playing.catch(() => this.fallBackToSpeakers());
    }
    // Without it there is no notification, but the sound is unaffected, so a refusal is not an error.
    await keeping?.catch(() => undefined);

    this.resetClock();
  }

  /**
   * Called when the phone pauses playback by itself, for example when another app starts playing.
   * Not called for `suspend`, which is a pause somebody asked for.
   */
  onInterrupted(handler: (() => void) | null): void {
    this.interruptedHandler = handler;
  }

  /** Stop playing and release the scheduled buffers, keeping the graph for a quick restart. */
  async suspend(): Promise<void> {
    this.wantsToPlay = false;
    this.flush();
    this.output?.pause();
    this.keeper?.pause();
    if (this.context && this.context.state === 'running') {
      await this.context.suspend();
    }
  }

  /** Tear the graph down completely. */
  async close(): Promise<void> {
    this.wantsToPlay = false;
    this.flush();
    document.removeEventListener('visibilitychange', this.onVisibilityChange);
    if (this.output) {
      this.output.onpause = null;
      this.output.pause();
      this.output.srcObject = null;
    }
    if (this.keeper) {
      this.keeper.onpause = null;
      this.keeper.pause();
      URL.revokeObjectURL(this.keeper.src);
      this.keeper = null;
    }
    const context = this.context;
    this.context = null;
    this.gainNode = null;
    this.analyserNode = null;
    this.output = null;
    if (context) {
      context.onstatechange = null;
      await context.close().catch(() => undefined);
    }
  }

  /** Whether sound is going through a media element, which is what lets it play with the screen off. */
  get playsAsMedia(): boolean {
    return this.output !== null;
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
   * Set the playback rate for buffers queued from now on.
   *
   * Buffers already scheduled keep the rate they were queued with. Restarting them would mean a gap on
   * every speed change, and the amount of audio affected is one jitter buffer.
   */
  setSpeed(speed: number): void {
    this.speed = Number.isFinite(speed) && speed > 0 ? Math.min(Math.max(speed, 0.1), 8) : 1;
  }

  get playbackSpeed(): number {
    return this.speed;
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
    source.playbackRate.value = this.speed;
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

    // Media time runs at the playback rate, so a second of wall clock is two seconds of recording at
    // double speed. Using the rate the anchor was queued at keeps the playhead honest across a change.
    const elapsedSeconds = context.currentTime - this.anchorContextTime;
    return this.anchorMediaMs + elapsedSeconds * 1000 * this.anchorSpeed;
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
      speed: this.speed,
    };
  }

  /**
   * Send the graph's output through a hidden media element, or straight to the speakers if this browser
   * has no media stream destination.
   */
  private routeOutput(context: AudioContext, last: AudioNode): HTMLAudioElement | null {
    if (typeof context.createMediaStreamDestination !== 'function' || typeof Audio === 'undefined') {
      last.connect(context.destination);
      return null;
    }
    const stream = context.createMediaStreamDestination();
    last.connect(stream);
    const element = new Audio();
    element.srcObject = stream.stream;
    element.onpause = () => this.pausedByPhone();
    return element;
  }

  private createKeeper(): HTMLAudioElement {
    const element = new Audio(URL.createObjectURL(new Blob([silentWav()], { type: 'audio/wav' })));
    element.loop = true;
    element.onpause = () => this.pausedByPhone();
    return element;
  }

  /**
   * A pause nobody asked for is the phone taking the audio away. Let the transport know, so the play
   * button tells the truth when the listener comes back to the page.
   */
  private pausedByPhone(): void {
    if (this.wantsToPlay) {
      this.wantsToPlay = false;
      this.interruptedHandler?.();
    }
  }

  /** The element refused to play, so play to the speakers the old way. Sound, if not in the background. */
  private fallBackToSpeakers(): void {
    const context = this.context;
    const analyserNode = this.analyserNode;
    if (!context || !analyserNode || !this.output) {
      return;
    }
    this.output.onpause = null;
    this.output.srcObject = null;
    this.output = null;
    analyserNode.disconnect();
    analyserNode.connect(context.destination);
  }

  /**
   * Try to get sound back after the phone interrupted it, when the page is visible again or the context
   * changed state. Best effort: some phones insist on a fresh tap, and then the play button is the way back.
   */
  private recover(): void {
    const context = this.context;
    if (!this.wantsToPlay || !context) {
      return;
    }
    if (context.state !== 'running' && context.state !== 'closed') {
      void context.resume().catch(() => undefined);
    }
    if (this.output?.paused) {
      void this.output.play().catch(() => undefined);
    }
    if (this.keeper?.paused) {
      void this.keeper.play().catch(() => undefined);
    }
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
