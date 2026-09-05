/**
 * Wire types.
 *
 * These mirror the Rust DTOs one for one and are the only place the API shape is written down on this
 * side. If the backend changes a field, this file is the single edit that surfaces every affected call
 * site as a type error.
 */

export type CaptureState = 'idle' | 'starting' | 'recording' | 'error';

export type Capture = {
  state: CaptureState;
  sessionId: number | null;
  deviceId: string | null;
  deviceName: string | null;
  sampleRate: number;
  channels: number;
  frameMs: number;
  startedAtMs: number | null;
  droppedFrames: number;
  error: string | null;
};

export type Levels = {
  rms: number;
  peak: number;
};

export type ServiceStatus = {
  capture: Capture;
  levels: Levels;
  listeners: number;
  serverTimeMs: number;
  liveEdgeMs: number | null;
};

export type Health = {
  status: string;
  version: string;
  uptimeMs: number;
};

export type InputDevice = {
  id: string;
  name: string;
  isDefault: boolean;
  isSelected: boolean;
  available: boolean;
  channels: number;
  sampleRate: number;
};

export type Settings = {
  inputDeviceId: string | null;
  gain: number;
  segmentSeconds: number;
  retentionHours: number;
  autoStart: boolean;
  frameMs: number;
};

export type SettingsPatch = Partial<Settings>;

export type CoverageBand = {
  startMs: number;
  endMs: number;
};

export type TimelineRange = {
  earliestMs: number | null;
  latestMs: number | null;
  liveEdgeMs: number | null;
  serverTimeMs: number;
  coverage: CoverageBand[];
};

export type Peaks = {
  fromMs: number;
  toMs: number;
  bucketMs: number;
  peaks: number[];
};

export type RecordingSession = {
  id: number;
  deviceId: string;
  deviceName: string;
  sampleRate: number;
  channels: number;
  startedAtMs: number;
  endedAtMs: number | null;
  segmentCount: number;
  bytes: number;
};

export type Storage = {
  bytes: number;
  segmentCount: number;
  oldestMs: number | null;
  newestMs: number | null;
  retentionHours: number;
  dataDir: string;
};

/** Playback state of the audio socket. */
export type StreamMode = 'live' | 'playback' | 'paused';

export type StreamInfoMessage = {
  type: 'stream-info';
  sampleRate: number;
  channels: number;
  frameMs: number;
  mode: StreamMode;
  serverTimeMs: number;
  liveEdgeMs: number | null;
  earliestMs: number | null;
  capturing: boolean;
};

export type ServerMessage =
  | StreamInfoMessage
  | { type: 'mode'; mode: StreamMode; positionMs: number }
  | { type: 'switched-to-live'; timestampMs: number }
  | { type: 'gap'; fromMs: number; toMs: number }
  | { type: 'end-of-recording'; timestampMs: number }
  | { type: 'level'; rms: number; peak: number }
  | { type: 'pong'; clientTimeMs: number; serverTimeMs: number }
  | { type: 'error'; code: string; message: string };

export type ClientMessage =
  | { type: 'live' }
  | { type: 'seek'; timestampMs: number }
  | { type: 'pause' }
  | { type: 'resume' }
  | { type: 'ping'; clientTimeMs: number };

/** One decoded audio frame off the wire. */
export type AudioFrame = {
  timestampMs: number;
  sampleRate: number;
  channels: number;
  live: boolean;
  samples: Float32Array;
};
