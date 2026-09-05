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
  /** `null` means recordings are kept forever. */
  retentionHours: number | null;
  autoStart: boolean;
  frameMs: number;
  /** `null` records at the capture device's own rate, which is the best quality it offers. */
  recordingSampleRate: number | null;
  /** `null` means the default location under the data directory. */
  recordingsDir: string | null;
  /** Where segments are written right now, always absolute. Read only. */
  effectiveRecordingsDir: string;
};

/**
 * A partial update. `effectiveRecordingsDir` is derived server side and cannot be written, and the two
 * nullable fields mean something specific when sent explicitly as null: keep forever, and use the
 * default directory.
 */
export type SettingsPatch = Partial<Omit<Settings, 'effectiveRecordingsDir'>>;

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

/** One calendar day that holds recordings, as offered by the day picker. */
export type RecordingDay = {
  /** Local calendar day on the host, `YYYY-MM-DD`. */
  day: string;
  /** First and last moment actually recorded that day. */
  startMs: number;
  endMs: number;
  /** Local midnight bounds, for framing the whole day on the timeline. */
  dayStartMs: number;
  dayEndMs: number;
  segmentCount: number;
  bytes: number;
  /** Audio actually captured, which is less than the span if the recorder was stopped part way. */
  recordedMs: number;
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
  /** `null` when recordings are kept forever. */
  retentionHours: number | null;
  dataDir: string;
  recordingsDir: string;
  /** Bytes one hour of audio occupies at the format in use. Exact, since segments are raw PCM. */
  bytesPerHour: number;
  /** What a full retention window would occupy, or `null` when keeping forever. */
  projectedMaxBytes: number | null;
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
  | { type: 'speed'; value: number }
  | { type: 'pong'; clientTimeMs: number; serverTimeMs: number }
  | { type: 'error'; code: string; message: string };

export type ClientMessage =
  | { type: 'live' }
  | { type: 'seek'; timestampMs: number }
  | { type: 'pause' }
  | { type: 'resume' }
  | { type: 'speed'; value: number }
  | { type: 'ping'; clientTimeMs: number };

/** One decoded audio frame off the wire. */
export type AudioFrame = {
  timestampMs: number;
  sampleRate: number;
  channels: number;
  live: boolean;
  samples: Float32Array;
};

/** The outcome of trying a recordings directory without saving it. */
export type DirectoryTest = {
  ok: boolean;
  /** The absolute path the setting resolves to. */
  resolvedPath: string;
  exists: boolean;
  /** True when it is missing but would be created on save. */
  willCreate: boolean;
  readable: boolean;
  writable: boolean;
  message: string;
};
