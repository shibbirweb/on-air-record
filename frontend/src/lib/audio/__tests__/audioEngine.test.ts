/**
 * The playback scheduler, run against a fake Web Audio graph and a fake `<audio>` element.
 *
 * What can be checked without a browser is the arithmetic and the wiring: where each buffer is scheduled
 * (one jitter buffer ahead, then back to back), when the clock resyncs, how the playhead is read off the
 * audio clock, how gain changes ramp, and the output routing CLAUDE.md describes (through a hidden media
 * element, a silent keeper clip except on WebKit, and the fall back to the speakers when the element is
 * refused). The fake context's clock only moves when a test moves it, which is what makes the schedule
 * exact. Whether it sounds right is for a person listening.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AudioFrame } from '@/api/types';

import { AudioEngine } from '../audioEngine';

class FakeNode {
  connections: unknown[] = [];
  disconnects = 0;
  connect(target: unknown) {
    this.connections.push(target);
    return target;
  }
  disconnect() {
    this.disconnects += 1;
    this.connections = [];
  }
}

class FakeParam {
  value = 1;
  ramps: [number, number, number][] = [];
  setTargetAtTime(target: number, startTime: number, timeConstant: number) {
    this.ramps.push([target, startTime, timeConstant]);
    this.value = target;
  }
}

class FakeGain extends FakeNode {
  gain = new FakeParam();
}

class FakeAnalyser extends FakeNode {
  fftSize = 0;
  smoothingTimeConstant = 0;
}

class FakeBuffer {
  readonly channels: Float32Array[];
  readonly numberOfChannels: number;
  readonly length: number;
  readonly sampleRate: number;
  constructor(numberOfChannels: number, length: number, sampleRate: number) {
    this.numberOfChannels = numberOfChannels;
    this.length = length;
    this.sampleRate = sampleRate;
    this.channels = Array.from({ length: numberOfChannels }, () => new Float32Array(length));
  }
  get duration() {
    return this.length / this.sampleRate;
  }
  getChannelData(channel: number) {
    return this.channels[channel];
  }
}

class FakeSource extends FakeNode {
  buffer: FakeBuffer | null = null;
  playbackRate = { value: 1 };
  startedAt: number | null = null;
  stopCalls = 0;
  throwOnStop = false;
  onended: (() => void) | null = null;
  start(when: number) {
    this.startedAt = when;
  }
  stop() {
    this.stopCalls += 1;
    if (this.throwOnStop) {
      throw new Error('InvalidStateError');
    }
  }
}

class FakeStreamDestination extends FakeNode {
  stream = { id: 'graph-output' };
}

class FakeAudioContext {
  static instances: FakeAudioContext[] = [];
  /** Remove to model a browser without media stream destinations. */
  static withStreamDestination = true;

  state: AudioContextState = 'suspended';
  currentTime = 0;
  sampleRate = 48_000;
  destination = new FakeNode();
  onstatechange: (() => void) | null = null;
  sources: FakeSource[] = [];
  gains: FakeGain[] = [];
  analysers: FakeAnalyser[] = [];
  streamDestinations: FakeStreamDestination[] = [];
  resumeCalls = 0;
  suspendCalls = 0;
  closeCalls = 0;
  failClose = false;
  createMediaStreamDestination?: () => FakeStreamDestination;
  readonly options: unknown;

  constructor(options: unknown) {
    this.options = options;
    FakeAudioContext.instances.push(this);
    if (FakeAudioContext.withStreamDestination) {
      this.createMediaStreamDestination = () => {
        const destination = new FakeStreamDestination();
        this.streamDestinations.push(destination);
        return destination;
      };
    }
  }

  async resume() {
    this.resumeCalls += 1;
    this.state = 'running';
  }
  async suspend() {
    this.suspendCalls += 1;
    this.state = 'suspended';
  }
  async close() {
    this.closeCalls += 1;
    this.state = 'closed';
    if (this.failClose) {
      throw new Error('already closed');
    }
  }
  createGain() {
    const gain = new FakeGain();
    this.gains.push(gain);
    return gain;
  }
  createAnalyser() {
    const analyser = new FakeAnalyser();
    this.analysers.push(analyser);
    return analyser;
  }
  createBuffer(channels: number, length: number, sampleRate: number) {
    return new FakeBuffer(channels, length, sampleRate);
  }
  createBufferSource() {
    const source = new FakeSource();
    this.sources.push(source);
    return source;
  }
}

class FakeAudio {
  static instances: FakeAudio[] = [];
  /** How the next `play()` on any element settles. */
  static refuse = new Set<'output' | 'keeper'>();

  srcObject: unknown = null;
  loop = false;
  paused = true;
  playCalls = 0;
  onpause: (() => void) | null = null;
  readonly src: string;

  constructor(src = '') {
    this.src = src;
    FakeAudio.instances.push(this);
  }

  get role(): 'output' | 'keeper' {
    return this.src === '' ? 'output' : 'keeper';
  }

  play() {
    this.playCalls += 1;
    if (FakeAudio.refuse.has(this.role)) {
      return Promise.reject(new Error('NotAllowedError'));
    }
    this.paused = false;
    return Promise.resolve();
  }

  pause() {
    this.paused = true;
    this.onpause?.();
  }
}

const CHROME =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36';
const IPHONE =
  'Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Mobile/15E148 Safari/604.1';

let visibilityListeners: (() => void)[] = [];
let documentStub: {
  visibilityState: string;
  addEventListener: ReturnType<typeof vi.fn>;
  removeEventListener: ReturnType<typeof vi.fn>;
};
let navigatorStub: { userAgent: string; audioSession?: { type: string } };
let revoked: string[] = [];

beforeEach(() => {
  FakeAudioContext.instances = [];
  FakeAudioContext.withStreamDestination = true;
  FakeAudio.instances = [];
  FakeAudio.refuse = new Set();
  visibilityListeners = [];
  revoked = [];
  documentStub = {
    visibilityState: 'visible',
    addEventListener: vi.fn((type: string, listener: () => void) => {
      if (type === 'visibilitychange') {
        visibilityListeners.push(listener);
      }
    }),
    removeEventListener: vi.fn((type: string, listener: () => void) => {
      if (type === 'visibilitychange') {
        visibilityListeners = visibilityListeners.filter((entry) => entry !== listener);
      }
    }),
  };
  navigatorStub = { userAgent: CHROME };
  vi.stubGlobal('AudioContext', FakeAudioContext);
  vi.stubGlobal('Audio', FakeAudio);
  vi.stubGlobal('document', documentStub);
  vi.stubGlobal('navigator', navigatorStub);
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:silence');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation((url: string) => {
    revoked.push(url);
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function context(): FakeAudioContext {
  const latest = FakeAudioContext.instances.at(-1);
  if (!latest) {
    throw new Error('no context was created');
  }
  return latest;
}

function output(): FakeAudio {
  const element = FakeAudio.instances.find((entry) => entry.role === 'output');
  if (!element) {
    throw new Error('no output element');
  }
  return element;
}

function keeper(): FakeAudio | undefined {
  return FakeAudio.instances.find((entry) => entry.role === 'keeper');
}

/** A 100 ms frame of `channels` interleaved channels at `sampleRate`. */
function frame(timestampMs: number, options: { channels?: number; sampleRate?: number; samples?: number[] } = {}): AudioFrame {
  const channels = options.channels ?? 1;
  const sampleRate = options.sampleRate ?? 48_000;
  const samples = options.samples ?? new Array((sampleRate / 10) * channels).fill(0);
  return { timestampMs, sampleRate, channels, live: true, samples: Float32Array.from(samples) };
}

function startTimes(): (number | null)[] {
  return context().sources.map((source) => source.startedAt);
}

async function started(): Promise<AudioEngine> {
  const engine = new AudioEngine();
  await engine.start();
  return engine;
}

describe('start', () => {
  it('builds the graph once, for interactive latency, with a steady analyser', async () => {
    const engine = await started();
    await engine.start();

    expect(FakeAudioContext.instances).toHaveLength(1);
    expect(context().options).toEqual({ latencyHint: 'interactive' });
    const [gain] = context().gains;
    const [analyser] = context().analysers;
    expect(gain.connections).toEqual([analyser]);
    expect(analyser.fftSize).toBe(2048);
    expect(analyser.smoothingTimeConstant).toBe(0.6);
    expect(engine.analyser).toBe(analyser);
  });

  it('routes the output through a hidden media element fed from a media stream', async () => {
    const engine = await started();
    const [analyser] = context().analysers;
    const [destination] = context().streamDestinations;

    expect(analyser.connections).toEqual([destination]);
    expect(output().srcObject).toBe(destination.stream);
    expect(output().paused).toBe(false);
    expect(engine.playsAsMedia).toBe(true);
    expect(context().destination.connections).toEqual([]);
  });

  it('resumes the context and reports running', async () => {
    const engine = new AudioEngine();
    expect(engine.running).toBe(false);
    await engine.start();
    expect(context().resumeCalls).toBe(1);
    expect(engine.running).toBe(true);
  });

  it('does not resume a context that is already running', async () => {
    const engine = await started();
    await engine.start();
    expect(context().resumeCalls).toBe(1);
    expect(output().playCalls).toBe(2);
  });

  it('plays straight to the speakers where there is no media stream destination', async () => {
    FakeAudioContext.withStreamDestination = false;
    const engine = await started();
    const [analyser] = context().analysers;

    expect(analyser.connections).toEqual([context().destination]);
    expect(FakeAudio.instances).toHaveLength(0);
    expect(engine.playsAsMedia).toBe(false);
  });

  it('falls back to the speakers when the media element refuses to play', async () => {
    FakeAudio.refuse.add('output');
    const engine = await started();
    const [analyser] = context().analysers;

    expect(analyser.disconnects).toBe(1);
    expect(analyser.connections).toEqual([context().destination]);
    expect(output().srcObject).toBeNull();
    expect(output().onpause).toBeNull();
    expect(engine.playsAsMedia).toBe(false);
    expect(engine.running).toBe(true);
  });

  it('starts the media element and the keeper before waiting on anything, inside the tap', async () => {
    // Phones refuse media that begins after an await: the tap that asked for it is over by then.
    const engine = new AudioEngine();
    const starting = engine.start();
    expect(output().playCalls).toBe(1);
    expect(keeper()?.playCalls).toBe(1);
    await starting;
  });

  it('never mutes the keeper clip, which browsers would then stop treating as media', async () => {
    await started();
    const clip = keeper() as unknown as { muted?: boolean; volume?: number };
    expect(clip.muted).not.toBe(true);
    expect(clip.volume).not.toBe(0);
  });

  it('plays a looping silent keeper clip beside the element outside WebKit', async () => {
    await started();
    const clip = keeper();
    expect(clip).toBeDefined();
    expect(clip?.src).toBe('blob:silence');
    expect(clip?.loop).toBe(true);
    expect(clip?.paused).toBe(false);
  });

  it('plays no keeper clip on WebKit, which shows controls for the stream itself', async () => {
    navigatorStub.userAgent = IPHONE;
    await started();
    expect(keeper()).toBeUndefined();
  });

  it('plays no keeper clip when there is no media element to keep company', async () => {
    FakeAudioContext.withStreamDestination = false;
    await started();
    expect(keeper()).toBeUndefined();
  });

  it('carries on with sound when the keeper clip is refused', async () => {
    FakeAudio.refuse.add('keeper');
    const engine = await started();
    expect(engine.playsAsMedia).toBe(true);
    expect(engine.running).toBe(true);
  });

  it('declares media playback to Safari when the audio session exists', async () => {
    navigatorStub.audioSession = { type: 'auto' };
    await started();
    expect(navigatorStub.audioSession.type).toBe('playback');
  });

  it('carries on when the audio session refuses the type', async () => {
    navigatorStub.audioSession = {
      get type() {
        return 'auto';
      },
      set type(_value: string) {
        throw new Error('read only');
      },
    };
    await expect(started()).resolves.toBeInstanceOf(AudioEngine);
  });

  it('starts the gain at the volume set before starting, or silent when muted', async () => {
    const engine = new AudioEngine();
    engine.setVolume(0.4);
    await engine.start();
    expect(context().gains[0].gain.value).toBe(0.4);

    const muted = new AudioEngine();
    muted.setMuted(true);
    await muted.start();
    expect(context().gains[0].gain.value).toBe(0);
  });
});

describe('enqueue', () => {
  it('ignores frames before the graph exists, or while the context is not running', async () => {
    const engine = new AudioEngine();
    engine.enqueue(frame(1_000));
    expect(FakeAudioContext.instances).toHaveLength(0);

    await engine.start();
    context().state = 'suspended';
    engine.enqueue(frame(1_000));
    expect(context().sources).toHaveLength(0);
  });

  it('ignores a frame with no samples', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000, { samples: [] }));
    expect(context().sources).toHaveLength(0);
  });

  it('schedules the first buffer one jitter buffer ahead, then each one where the last ended', async () => {
    const engine = await started();
    context().currentTime = 2;
    // A flush restarts the clock from now, as a seek does, so the first frame is not a resync.
    engine.flush();
    engine.enqueue(frame(1_000));
    engine.enqueue(frame(1_100));
    engine.enqueue(frame(1_200));

    const times = startTimes();
    expect(times[0]).toBeCloseTo(2.15, 10);
    expect(times[1]).toBeCloseTo(2.25, 10);
    expect(times[2]).toBeCloseTo(2.35, 10);
    expect(engine.stats().resyncs).toBe(0);
  });

  it('connects every buffer to the gain at real time by default', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000));
    const [source] = context().sources;
    expect(source.connections).toEqual([context().gains[0]]);
    expect(source.playbackRate.value).toBe(1);
  });

  it('plays each buffer at the speed set when it was queued', async () => {
    const engine = await started();
    engine.setSpeed(2);
    engine.enqueue(frame(1_000));
    engine.setSpeed(0.5);
    engine.enqueue(frame(1_100));
    expect(context().sources.map((source) => source.playbackRate.value)).toEqual([2, 0.5]);
  });

  it('starts each buffer where the last one ends at the speed it was queued at', async () => {
    const engine = await started();
    context().currentTime = 2;
    engine.flush();
    engine.setSpeed(2);
    engine.enqueue(frame(1_000));
    engine.enqueue(frame(1_100));
    engine.setSpeed(0.5);
    engine.enqueue(frame(1_200));
    engine.enqueue(frame(1_300));

    // 100 ms frames take 50 ms at double speed and 200 ms at half speed.
    const times = startTimes();
    expect(times[0]).toBeCloseTo(2.15, 10);
    expect(times[1]).toBeCloseTo(2.2, 10);
    expect(times[2]).toBeCloseTo(2.25, 10);
    expect(times[3]).toBeCloseTo(2.45, 10);
  });

  it('keeps pace at double speed when frames arrive twice as fast, with no resync', async () => {
    const engine = await started();
    context().currentTime = 1;
    engine.flush();
    engine.setSpeed(2);
    // The server paces frames to the speed: one every 50 ms of wall clock.
    for (let index = 0; index < 100; index += 1) {
      engine.enqueue(frame(1_000 + index * 100));
      context().currentTime += 0.05;
    }
    expect(engine.stats().resyncs).toBe(0);
    expect(engine.stats().bufferedSeconds).toBeCloseTo(0.15, 6);
  });

  it('splits interleaved channels into the buffer, keeping the source sample rate', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000, { channels: 2, sampleRate: 44_100, samples: [0.1, -0.1, 0.2, -0.2, 0.3, -0.3] }));

    const buffer = context().sources[0].buffer as FakeBuffer;
    expect(buffer.sampleRate).toBe(44_100);
    expect(buffer.numberOfChannels).toBe(2);
    expect(Array.from(buffer.getChannelData(0))).toEqual([0.1, 0.2, 0.3].map(Math.fround));
    expect(Array.from(buffer.getChannelData(1))).toEqual([-0.1, -0.2, -0.3].map(Math.fround));
  });

  it('treats a frame claiming no channels as mono', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000, { channels: 0, samples: [0.5, 0.25] }));
    const buffer = context().sources[0].buffer as FakeBuffer;
    expect(buffer.numberOfChannels).toBe(1);
    expect(buffer.length).toBe(2);
  });

  it('restarts the clock one jitter buffer ahead when the queue ran dry', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000));
    context().currentTime = 5;
    engine.enqueue(frame(1_100));

    expect(startTimes()[1]).toBeCloseTo(5.15, 10);
    expect(engine.stats().resyncs).toBe(1);
  });

  it('pulls the clock back when it has crept more than six jitter buffers ahead', async () => {
    const engine = await started();
    // Eight 100 ms frames put the next start at 0.95 s, past the 0.9 s limit, while the clock stands still.
    for (let index = 0; index < 9; index += 1) {
      engine.enqueue(frame(1_000 + index * 100));
    }
    expect(startTimes()[7]).toBeCloseTo(0.85, 10);
    expect(startTimes()[8]).toBeCloseTo(0.15, 10);
    expect(engine.stats().resyncs).toBe(1);
  });

  it('uses the jitter buffer that was set, within its limits', async () => {
    const engine = await started();
    engine.setJitterSeconds(0.5);
    engine.flush();
    engine.enqueue(frame(1_000));
    expect(startTimes()[0]).toBeCloseTo(0.5, 10);

    engine.setJitterSeconds(0);
    engine.flush();
    engine.enqueue(frame(1_000));
    expect(startTimes()[1]).toBeCloseTo(0.03, 10);

    engine.setJitterSeconds(10);
    engine.flush();
    engine.enqueue(frame(1_000));
    expect(startTimes()[2]).toBeCloseTo(2, 10);
  });
});

describe('currentPlayheadMs', () => {
  it('is null before the graph exists and before anything is queued', async () => {
    const engine = new AudioEngine();
    expect(engine.currentPlayheadMs()).toBeNull();
    await engine.start();
    expect(engine.currentPlayheadMs()).toBeNull();
  });

  it('reads what is coming out of the speakers off the audio clock, not the newest frame', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000_000));
    engine.enqueue(frame(1_000_100));

    // The newest frame starts at 0.25 s; at 0.15 s the first one is just beginning.
    context().currentTime = 0.15;
    expect(engine.currentPlayheadMs()).toBeCloseTo(1_000_000, 6);
    context().currentTime = 0.2;
    expect(engine.currentPlayheadMs()).toBeCloseTo(1_000_050, 6);
    context().currentTime = 0.3;
    expect(engine.currentPlayheadMs()).toBeCloseTo(1_000_150, 6);
  });

  it('moves at the playback speed, so it keeps up with what is heard at double speed', async () => {
    const engine = await started();
    engine.setSpeed(2);
    engine.enqueue(frame(1_000_000));

    // Queued at 0.15 s; a tenth of a second later, two tenths of recording have played.
    context().currentTime = 0.25;
    expect(engine.currentPlayheadMs()).toBeCloseTo(1_000_200, 6);
  });

  it('is null again after a flush, until the next frame is queued', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000_000));
    engine.flush();
    expect(engine.currentPlayheadMs()).toBeNull();
  });

  it('is null after the graph is closed', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000_000));
    await engine.close();
    expect(engine.currentPlayheadMs()).toBeNull();
  });
});

describe('flush', () => {
  it('stops every scheduled buffer and restarts the clock from now', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000));
    engine.enqueue(frame(1_100));
    context().currentTime = 0.2;
    engine.flush();

    expect(context().sources.map((source) => source.stopCalls)).toEqual([1, 1]);
    expect(context().sources.every((source) => source.onended === null)).toBe(true);
    engine.enqueue(frame(5_000));
    expect(startTimes()[2]).toBeCloseTo(0.35, 10);
  });

  it('leaves alone a buffer that already finished', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000));
    engine.enqueue(frame(1_100));
    context().sources[0].onended?.();
    engine.flush();
    expect(context().sources.map((source) => source.stopCalls)).toEqual([0, 1]);
  });

  it('carries on past a buffer that refuses to stop', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000));
    engine.enqueue(frame(1_100));
    context().sources[0].throwOnStop = true;
    expect(() => engine.flush()).not.toThrow();
    expect(context().sources[1].stopCalls).toBe(1);
    // Stopped buffers are forgotten, so a second flush has nothing to stop.
    engine.flush();
    expect(context().sources.map((source) => source.stopCalls)).toEqual([1, 1]);
  });

  it('is safe before the graph exists', () => {
    expect(() => new AudioEngine().flush()).not.toThrow();
  });
});

describe('volume and mute', () => {
  it('ramps the gain rather than stepping it, from the current audio time', async () => {
    const engine = await started();
    context().currentTime = 3;
    engine.setVolume(0.5);
    expect(context().gains[0].gain.ramps.at(-1)).toEqual([0.5, 3, 0.015]);
  });

  it('clamps the volume between silent and full', async () => {
    const engine = await started();
    engine.setVolume(4);
    expect(context().gains[0].gain.value).toBe(1);
    engine.setVolume(-1);
    expect(context().gains[0].gain.value).toBe(0);
  });

  it('silences while muted and returns to the volume when unmuted', async () => {
    const engine = await started();
    engine.setVolume(0.7);
    engine.setMuted(true);
    expect(context().gains[0].gain.value).toBe(0);
    engine.setVolume(0.3);
    expect(context().gains[0].gain.value).toBe(0);
    engine.setMuted(false);
    expect(context().gains[0].gain.value).toBe(0.3);
  });
});

describe('setSpeed', () => {
  it('keeps the speed within a tenth and eight times', () => {
    const engine = new AudioEngine();
    engine.setSpeed(2);
    expect(engine.playbackSpeed).toBe(2);
    engine.setSpeed(0.01);
    expect(engine.playbackSpeed).toBe(0.1);
    engine.setSpeed(100);
    expect(engine.playbackSpeed).toBe(8);
  });

  it('falls back to real time for a speed that makes no sense', () => {
    const engine = new AudioEngine();
    for (const nonsense of [0, -2, Number.NaN, Number.POSITIVE_INFINITY]) {
      engine.setSpeed(3);
      engine.setSpeed(nonsense);
      expect(engine.playbackSpeed).toBe(1);
    }
  });
});

describe('stats', () => {
  it('reports nothing running before the graph exists', () => {
    expect(new AudioEngine().stats()).toEqual({
      running: false,
      bufferedSeconds: 0,
      scheduledFrames: 0,
      droppedFrames: 0,
      resyncs: 0,
      sampleRate: 0,
      speed: 1,
    });
  });

  it('reports how much is queued ahead of the clock, never less than nothing', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000));
    engine.enqueue(frame(1_100));
    context().currentTime = 0.05;

    const stats = engine.stats();
    expect(stats.running).toBe(true);
    expect(stats.bufferedSeconds).toBeCloseTo(0.3, 10);
    expect(stats.scheduledFrames).toBe(2);
    expect(stats.sampleRate).toBe(48_000);

    context().currentTime = 10;
    expect(engine.stats().bufferedSeconds).toBe(0);
  });
});

describe('suspend', () => {
  it('drops the queue, pauses both elements and suspends the context, keeping the graph', async () => {
    const engine = await started();
    engine.enqueue(frame(1_000));
    await engine.suspend();

    expect(context().sources[0].stopCalls).toBe(1);
    expect(output().paused).toBe(true);
    expect(keeper()?.paused).toBe(true);
    expect(context().suspendCalls).toBe(1);
    expect(engine.running).toBe(false);

    await engine.start();
    expect(FakeAudioContext.instances).toHaveLength(1);
    expect(engine.running).toBe(true);
  });

  it('does not count a pause somebody asked for as an interruption', async () => {
    const engine = await started();
    const interrupted = vi.fn();
    engine.onInterrupted(interrupted);
    await engine.suspend();
    expect(interrupted).not.toHaveBeenCalled();
  });

  it('does not suspend a context that is not running, and is safe before start', async () => {
    await expect(new AudioEngine().suspend()).resolves.toBeUndefined();
    const engine = await started();
    context().state = 'interrupted' as AudioContextState;
    await engine.suspend();
    expect(context().suspendCalls).toBe(0);
  });
});

describe('close', () => {
  it('tears the graph down, releasing the elements, the clip and the visibility listener', async () => {
    const engine = await started();
    const element = output();
    await engine.close();

    expect(context().closeCalls).toBe(1);
    expect(element.srcObject).toBeNull();
    expect(element.onpause).toBeNull();
    expect(element.paused).toBe(true);
    expect(revoked).toEqual(['blob:silence']);
    expect(visibilityListeners).toEqual([]);
    expect(context().onstatechange).toBeNull();
    expect(engine.running).toBe(false);
    expect(engine.playsAsMedia).toBe(false);
    expect(engine.analyser).toBeNull();
  });

  it('carries on when the context fails to close', async () => {
    const engine = await started();
    context().failClose = true;
    await expect(engine.close()).resolves.toBeUndefined();
  });

  it('is safe before start, and builds a fresh graph if started again', async () => {
    const engine = new AudioEngine();
    await expect(engine.close()).resolves.toBeUndefined();
    await engine.start();
    await engine.close();
    await engine.start();
    expect(FakeAudioContext.instances).toHaveLength(2);
  });
});

describe('interruptions', () => {
  it('tells the transport once when the phone pauses the element on its own', async () => {
    const engine = await started();
    const interrupted = vi.fn();
    engine.onInterrupted(interrupted);

    output().pause();
    output().pause();
    expect(interrupted).toHaveBeenCalledTimes(1);
  });

  it('treats the keeper clip being paused the same way', async () => {
    const engine = await started();
    const interrupted = vi.fn();
    engine.onInterrupted(interrupted);
    keeper()?.pause();
    expect(interrupted).toHaveBeenCalledTimes(1);
  });

  it('tells nobody once the handler is removed', async () => {
    const engine = await started();
    const interrupted = vi.fn();
    engine.onInterrupted(interrupted);
    engine.onInterrupted(null);
    output().pause();
    expect(interrupted).not.toHaveBeenCalled();
  });
});

describe('recovering after an interruption', () => {
  it('resumes the context and replays the elements when the page is visible again', async () => {
    await started();
    context().state = 'interrupted' as AudioContextState;
    output().paused = true;
    const clip = keeper();
    if (clip) {
      clip.paused = true;
    }

    visibilityListeners.forEach((listener) => listener());
    expect(context().resumeCalls).toBe(2);
    expect(output().playCalls).toBe(2);
    expect(keeper()?.playCalls).toBe(2);
  });

  it('tries again when the context changes state', async () => {
    await started();
    context().state = 'suspended';
    context().onstatechange?.();
    expect(context().resumeCalls).toBe(2);
  });

  it('does nothing while the page is hidden', async () => {
    await started();
    context().state = 'suspended';
    documentStub.visibilityState = 'hidden';
    visibilityListeners.forEach((listener) => listener());
    expect(context().resumeCalls).toBe(1);
  });

  it('does nothing after a pause somebody asked for', async () => {
    const engine = await started();
    await engine.suspend();
    visibilityListeners.forEach((listener) => listener());
    expect(context().resumeCalls).toBe(1);
    expect(output().playCalls).toBe(1);
  });

  it('never tries to resume a closed context, and leaves playing elements alone', async () => {
    await started();
    context().state = 'closed';
    context().onstatechange?.();
    expect(context().resumeCalls).toBe(1);
    expect(output().playCalls).toBe(1);
  });

  it('swallows a refusal to resume or replay', async () => {
    await started();
    context().state = 'suspended';
    vi.spyOn(context(), 'resume').mockRejectedValue(new Error('needs a gesture'));
    FakeAudio.refuse.add('output');
    output().paused = true;
    expect(() => visibilityListeners.forEach((listener) => listener())).not.toThrow();
    await Promise.resolve();
  });
});
