/**
 * What the server sends, checked against the types in `types.ts` through the golden fixtures the backend
 * generates in `contracts/`: every stream control message, every REST answer the page reads, the error
 * envelope, and the string enums.
 *
 * The backend test fails when its serialised output drifts from those files, and this one fails when the
 * TypeScript does. Between them a field renamed, added or removed on either side cannot pass both suites.
 * Each check is exact in both directions and recursive: a key the server sends and the type lacks fails,
 * so does a key the type has and the server never sends, and so does a `null` where the type does not
 * allow one. The shape tables below are typed from the TypeScript types, so the compiler forces them to
 * list every key; the runtime compares them with the fixtures.
 *
 * When one fails after a deliberate backend change, update `types.ts` to match the regenerated fixture.
 */

import { describe, expect, it } from 'vitest';

import {
  checkObject,
  Coverage,
  fixtures,
  members,
  sameNames,
  shape,
} from '@/test/contract';
import type { AnyShape, Shape } from '@/test/contract';

import type {
  AuthMode,
  AuthState,
  Bookmark,
  BookmarkList,
  Capture,
  CaptureState,
  DeviceList,
  DirectoryTest,
  ErrorEnvelope,
  ExportPlan,
  Health,
  InputDevice,
  InstallInfo,
  ListenerView,
  MetricsToken,
  NewMetricsToken,
  NextSound,
  Peaks,
  PlayerState,
  RecordingDay,
  RecordingDayList,
  RecordingSession,
  RecoveryCodes,
  ReleaseInfo,
  Role,
  ServerMessage,
  ServiceStatus,
  SessionList,
  Settings,
  Sound,
  SoundSensitivity,
  SoundsWindow,
  Storage,
  StreamMode,
  TimelineRange,
  TwoFactorSetup,
  TwoFactorStatus,
  UpdateStatus,
  User,
  UserList,
} from '../types';

// Nested shapes, shared by the tables below.

const captureShape: Shape<Capture> = {
  state: 'string',
  sessionId: { nullable: 'number' },
  deviceId: { nullable: 'string' },
  deviceName: { nullable: 'string' },
  sampleRate: 'number',
  deviceSampleRate: 'number',
  channels: 'number',
  frameMs: 'number',
  startedAtMs: { nullable: 'number' },
  droppedFrames: 'number',
  error: { nullable: 'string' },
};

const userShape: Shape<User> = {
  id: 'number',
  email: 'string',
  role: 'string',
  createdAtMs: 'number',
  twoFactorEnabled: 'boolean',
};

const bookmarkShape: Shape<Bookmark> = {
  id: 'number',
  timestampMs: 'number',
  label: 'string',
  note: { nullable: 'string' },
  createdAtMs: 'number',
};

const soundShape: Shape<Sound> = {
  startMs: 'number',
  endMs: 'number',
  seekMs: 'number',
  peak: 'number',
};

const releaseShape: Shape<ReleaseInfo> = {
  version: 'string',
  tag: 'string',
  prerelease: 'boolean',
  publishedAtMs: { nullable: 'number' },
  notes: 'string',
  url: 'string',
};

const installShape: Shape<InstallInfo> = {
  kind: 'string',
  dir: { nullable: 'string' },
  os: 'string',
  target: 'string',
};

const deviceShape: Shape<InputDevice> = {
  id: 'string',
  name: 'string',
  isDefault: 'boolean',
  isSelected: 'boolean',
  available: 'boolean',
  channels: 'number',
  sampleRate: 'number',
};

const sessionShape: Shape<RecordingSession> = {
  id: 'number',
  deviceId: 'string',
  deviceName: 'string',
  sampleRate: 'number',
  channels: 'number',
  startedAtMs: 'number',
  endedAtMs: { nullable: 'number' },
  segmentCount: 'number',
  bytes: 'number',
};

const dayShape: Shape<RecordingDay> = {
  day: 'string',
  startMs: 'number',
  endMs: 'number',
  dayStartMs: 'number',
  dayEndMs: 'number',
  segmentCount: 'number',
  bytes: 'number',
  recordedMs: 'number',
};

const listenerShape: Shape<ListenerView> = {
  id: 'number',
  email: { nullable: 'string' },
  role: { nullable: 'string' },
  address: 'string',
  userAgent: { nullable: 'string' },
  connectedAtMs: 'number',
  activity: 'string',
  fromMs: { nullable: 'number' },
  player: 'string',
};

/** One entry per variant of the union, keyed by its `type`, so a variant cannot be left out. */
type ServerMessageShapes = { [M in ServerMessage as M['type']]: Shape<M> };

const serverMessageShapes: ServerMessageShapes = {
  'stream-info': {
    type: 'string',
    sampleRate: 'number',
    channels: 'number',
    frameMs: 'number',
    mode: 'string',
    serverTimeMs: 'number',
    liveEdgeMs: { nullable: 'number' },
    earliestMs: { nullable: 'number' },
    capturing: 'boolean',
  },
  mode: { type: 'string', mode: 'string', positionMs: 'number' },
  'switched-to-live': { type: 'string', timestampMs: 'number' },
  gap: { type: 'string', fromMs: 'number', toMs: 'number' },
  'end-of-recording': { type: 'string', timestampMs: 'number' },
  level: { type: 'string', rms: 'number', peak: 'number' },
  speed: { type: 'string', value: 'number' },
  pong: { type: 'string', clientTimeMs: 'number', serverTimeMs: 'number' },
  error: { type: 'string', code: 'string', message: 'string' },
  listeners: { type: 'string', listeners: { array: { object: listenerShape } } },
  'listeners-hidden': { type: 'string' },
};

/** Every REST answer the page reads, by the name the backend fixture files it under. */
const responseShapes: Record<string, AnyShape> = {
  health: shape<Health>({ status: 'string', version: 'string', uptimeMs: 'number' }),
  status: shape<ServiceStatus>({
    capture: { object: captureShape },
    levels: { object: { rms: 'number', peak: 'number' } },
    listeners: 'number',
    serverTimeMs: 'number',
    liveEdgeMs: { nullable: 'number' },
  }),
  settings: shape<Settings>({
    inputDeviceId: { nullable: 'string' },
    gain: 'number',
    segmentSeconds: 'number',
    retentionHours: { nullable: 'number' },
    autoStart: 'boolean',
    autoStartDelaySeconds: 'number',
    frameMs: 'number',
    recordingSampleRate: { nullable: 'number' },
    recordingsDir: { nullable: 'string' },
    effectiveRecordingsDir: 'string',
    checkForUpdates: 'boolean',
    soundSensitivity: 'string',
  }),
  directoryTest: shape<DirectoryTest>({
    ok: 'boolean',
    resolvedPath: 'string',
    exists: 'boolean',
    willCreate: 'boolean',
    readable: 'boolean',
    writable: 'boolean',
    message: 'string',
  }),
  storage: shape<Storage>({
    bytes: 'number',
    segmentCount: 'number',
    oldestMs: { nullable: 'number' },
    newestMs: { nullable: 'number' },
    retentionHours: { nullable: 'number' },
    dataDir: 'string',
    recordingsDir: 'string',
    bytesPerHour: 'number',
    projectedMaxBytes: { nullable: 'number' },
  }),
  sessions: shape<SessionList>({ sessions: { array: { object: sessionShape } } }),
  devices: shape<DeviceList>({ devices: { array: { object: deviceShape } } }),
  timelineRange: shape<TimelineRange>({
    earliestMs: { nullable: 'number' },
    latestMs: { nullable: 'number' },
    liveEdgeMs: { nullable: 'number' },
    serverTimeMs: 'number',
    coverage: { array: { object: { startMs: 'number', endMs: 'number' } } },
  }),
  recordingDays: shape<RecordingDayList>({ days: { array: { object: dayShape } } }),
  peaks: shape<Peaks>({
    fromMs: 'number',
    toMs: 'number',
    bucketMs: 'number',
    peaks: { array: 'number' },
  }),
  sounds: shape<SoundsWindow>({
    fromMs: 'number',
    toMs: 'number',
    sensitivity: 'string',
    sounds: { array: { object: soundShape } },
  }),
  nextSound: shape<NextSound>({ sound: { nullable: { object: soundShape } } }),
  bookmark: shape<Bookmark>(bookmarkShape),
  bookmarks: shape<BookmarkList>({ bookmarks: { array: { object: bookmarkShape } } }),
  exportPlan: shape<ExportPlan>({
    fromMs: 'number',
    toMs: 'number',
    durationMs: 'number',
    sampleRate: 'number',
    channels: 'number',
    totalBytes: 'number',
    mixedRates: 'boolean',
  }),
  authState: shape<AuthState>({
    mode: 'string',
    user: { nullable: { object: userShape } },
    pendingTwoFactor: 'boolean',
  }),
  user: shape<User>(userShape),
  users: shape<UserList>({ users: { array: { object: userShape } } }),
  twoFactorStatus: shape<TwoFactorStatus>({ enabled: 'boolean', recoveryCodesLeft: 'number' }),
  twoFactorSetup: shape<TwoFactorSetup>({
    secretKey: 'string',
    otpauthUri: 'string',
    qrSvg: 'string',
  }),
  recoveryCodes: shape<RecoveryCodes>({ recoveryCodes: { array: 'string' } }),
  metricsToken: shape<MetricsToken>({ createdAtMs: { nullable: 'number' } }),
  newMetricsToken: shape<NewMetricsToken>({ token: 'string', createdAtMs: 'number' }),
  updateStatus: shape<UpdateStatus>({
    currentVersion: 'string',
    channel: 'string',
    automatic: 'boolean',
    checkedAtMs: { nullable: 'number' },
    error: { nullable: 'string' },
    available: { nullable: { object: releaseShape } },
    releases: { array: { object: releaseShape } },
    install: { object: installShape },
    releasesUrl: 'string',
  }),
  error: shape<ErrorEnvelope>({ error: { object: { code: 'string', message: 'string' } } }),
};

/** Every string union on the wire, each listed in full, compared with every value the server can send. */
const enumMembers: Record<string, string[]> = {
  authMode: members<AuthMode>({ undecided: true, open: true, accounts: true }),
  captureState: members<CaptureState>({ idle: true, starting: true, recording: true, error: true }),
  channel: members<UpdateStatus['channel']>({ stable: true, beta: true }),
  installKind: members<InstallInfo['kind']>({
    installer: true,
    systemd: true,
    docker: true,
    manual: true,
  }),
  playerState: members<PlayerState>({ idle: true, playing: true, paused: true }),
  role: members<Role>({ admin: true, listener: true }),
  soundSensitivity: members<SoundSensitivity>({ low: true, medium: true, high: true }),
  streamMode: members<StreamMode>({ live: true, playback: true, paused: true }),
};

/** Where each enum appears in the fixtures, so the values sent there are checked against the union. */
const enumFields: [string, string, (example: Record<string, unknown>) => unknown[]][] = [
  ['captureState', 'status', (example) => [(example.capture as Capture).state]],
  ['soundSensitivity', 'settings', (example) => [example.soundSensitivity]],
  ['soundSensitivity', 'sounds', (example) => [example.sensitivity]],
  ['authMode', 'authState', (example) => [example.mode]],
  ['role', 'user', (example) => [example.role]],
  ['channel', 'updateStatus', (example) => [example.channel]],
  ['installKind', 'updateStatus', (example) => [(example.install as InstallInfo).kind]],
];

function checkGroups(
  file: string,
  shapes: Record<string, AnyShape>,
  examples: Record<string, Record<string, unknown>[]>,
): string[] {
  const problems = sameNames(file, Object.keys(shapes), Object.keys(examples));
  // Paths leave out which example they came from, because coverage is judged across all the examples
  // of a name together: one may show a field null and another set.
  const coverage = new Coverage();
  for (const [name, fields] of Object.entries(shapes)) {
    for (const example of examples[name] ?? []) {
      checkObject(example, fields, `${file} ${name}`, coverage, problems);
    }
  }
  return [...new Set(problems), ...coverage.gaps()];
}

describe('server messages', () => {
  it('have exactly the variants, keys and value kinds of the ServerMessage union', () => {
    expect(
      checkGroups(
        'server-messages.json',
        serverMessageShapes as unknown as Record<string, AnyShape>,
        fixtures.serverMessages,
      ),
    ).toEqual([]);
  });

  it('are filed under the type tag they carry', () => {
    for (const [name, examples] of Object.entries(fixtures.serverMessages)) {
      for (const example of examples) {
        expect(example.type).toBe(name);
      }
    }
  });

  it('carry modes, roles and player states the unions know', () => {
    const modes = enumMembers.streamMode;
    for (const example of fixtures.serverMessages['stream-info'] ?? []) {
      expect(modes).toContain(example.mode);
    }
    for (const example of fixtures.serverMessages.mode ?? []) {
      expect(modes).toContain(example.mode);
    }
    for (const example of fixtures.serverMessages.listeners ?? []) {
      for (const listener of example.listeners as ListenerView[]) {
        expect(modes).toContain(listener.activity);
        expect(enumMembers.playerState).toContain(listener.player);
        if (listener.role !== null) {
          expect(enumMembers.role).toContain(listener.role);
        }
      }
    }
  });
});

describe('REST responses', () => {
  it('have exactly the keys and value kinds of the types the client reads them as', () => {
    expect(checkGroups('responses.json', responseShapes, fixtures.responses)).toEqual([]);
  });

  it('carry enum values the TypeScript unions know', () => {
    for (const [union, response, pick] of enumFields) {
      for (const example of fixtures.responses[response] ?? []) {
        for (const value of pick(example)) {
          expect(enumMembers[union], `${response} sends ${String(value)}`).toContain(value);
        }
      }
    }
  });
});

describe('wire enums', () => {
  it('list exactly the values of each TypeScript union', () => {
    expect(Object.keys(enumMembers).sort()).toEqual(Object.keys(fixtures.enums).sort());
    for (const [name, values] of Object.entries(enumMembers)) {
      expect(values, name).toEqual([...(fixtures.enums[name] ?? [])].sort());
    }
  });
});
