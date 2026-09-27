/**
 * What the browser sends, checked against the fixtures the backend parses: `contracts/client-messages.json`
 * for the stream socket and `contracts/requests.json` for REST bodies and query strings.
 *
 * Those two files are written on this side. This test proves they are what the frontend really sends,
 * by calling every `api` method with the fixture's values against a stubbed `fetch` and comparing the
 * path, verb and body or query it produced, and by holding the `ClientMessage` union to the fixture's keys.
 * `backend/src/contract_tests.rs` then parses each one into the server's own request types and checks
 * every value arrived, so a field renamed here fails there until the server agrees.
 *
 * The table of `api` methods is typed over every method of `api`, so a new method cannot be added without
 * saying whether it sends a body, a query, or nothing, and one that sends something without a fixture fails.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  checkObject,
  checkPartial,
  Coverage,
  fixtures,
  sameNames,
} from '@/test/contract';
import type { AnyShape, Json, Shape } from '@/test/contract';

import { api } from '../client';
import { StreamSocket } from '../streamSocket';
import type { ClientMessage, PlayerState, Role, SettingsPatch } from '../types';

type ClientMessageShapes = { [M in ClientMessage as M['type']]: Shape<M> };

const clientMessageShapes: ClientMessageShapes = {
  live: { type: 'string' },
  seek: { type: 'string', timestampMs: 'number' },
  pause: { type: 'string' },
  resume: { type: 'string' },
  speed: { type: 'string', value: 'number' },
  ping: { type: 'string', clientTimeMs: 'number' },
  player: { type: 'string', state: 'string' },
};

const settingsPatchShape: Shape<SettingsPatch> = {
  inputDeviceId: { nullable: 'string' },
  gain: 'number',
  segmentSeconds: 'number',
  retentionHours: { nullable: 'number' },
  autoStart: 'boolean',
  autoStartDelaySeconds: 'number',
  frameMs: 'number',
  recordingSampleRate: { nullable: 'number' },
  recordingsDir: { nullable: 'string' },
  checkForUpdates: 'boolean',
  soundSensitivity: 'string',
};

type BookmarkPatch = NonNullable<Parameters<typeof api.updateBookmark>[1]>;

const bookmarkPatchShape: Shape<BookmarkPatch> = {
  label: 'string',
  note: { nullable: 'string' },
};

/** What each `api` method sends. Typed over all of `api`, so none can be left unclassified. */
const payloads: Record<keyof typeof api, 'body' | 'query' | 'none'> = {
  health: 'none',
  authState: 'none',
  chooseOpen: 'none',
  setUp: 'body',
  logIn: 'body',
  verifyLogin: 'body',
  logOut: 'none',
  twoFactorStatus: 'none',
  beginTwoFactorSetup: 'none',
  enableTwoFactor: 'body',
  disableTwoFactor: 'body',
  regenerateRecoveryCodes: 'body',
  resetUserTwoFactor: 'none',
  changePassword: 'body',
  users: 'none',
  createUser: 'body',
  updateUserRole: 'body',
  setUserPassword: 'body',
  deleteUser: 'none',
  status: 'none',
  startCapture: 'none',
  stopCapture: 'none',
  devices: 'none',
  selectDevice: 'body',
  settings: 'none',
  updateSettings: 'body',
  settingsDefaults: 'none',
  updates: 'none',
  checkForUpdates: 'none',
  testRecordingsDir: 'body',
  resetSettings: 'none',
  timelineRange: 'none',
  recordingDays: 'none',
  peaks: 'query',
  sounds: 'query',
  nextSound: 'query',
  bookmarks: 'none',
  createBookmark: 'body',
  updateBookmark: 'body',
  deleteBookmark: 'none',
  sessions: 'none',
  storage: 'none',
  exportPlan: 'query',
  exportUrl: 'query',
};

function text(values: Json | Record<string, unknown>, key: string): string {
  const value = values[key];
  if (typeof value !== 'string') {
    throw new Error(`${key} is ${JSON.stringify(value)}, not a string`);
  }
  return value;
}

function number(values: Json | Record<string, unknown>, key: string): number {
  const value = values[key];
  if (typeof value !== 'number') {
    throw new Error(`${key} is ${JSON.stringify(value)}, not a number`);
  }
  return value;
}

function textOrNull(values: Json, key: string): string | null {
  return values[key] === null ? null : text(values, key);
}

/** The id a path names, such as the 7 in `/users/7/password`, for the calls that take one. */
function idIn(path: string): number {
  const match = /\/(\d+)(\/|$)/.exec(path);
  if (!match) {
    throw new Error(`no id in ${path}`);
  }
  return Number(match[1]);
}

/** Each `api` method that sends a body, called with the fixture's own values. */
const bodyCalls: Record<string, (body: Json, path: string) => Promise<unknown>> = {
  setUp: (body) => api.setUp(text(body, 'email'), text(body, 'password')),
  logIn: (body) => api.logIn(text(body, 'email'), text(body, 'password')),
  verifyLogin: (body) => api.verifyLogin(text(body, 'code')),
  enableTwoFactor: (body) => api.enableTwoFactor(text(body, 'code')),
  disableTwoFactor: (body) => api.disableTwoFactor(text(body, 'password')),
  regenerateRecoveryCodes: (body) => api.regenerateRecoveryCodes(text(body, 'password')),
  changePassword: (body) =>
    api.changePassword(text(body, 'currentPassword'), text(body, 'newPassword')),
  createUser: (body) =>
    api.createUser(text(body, 'email'), text(body, 'password'), text(body, 'role') as Role),
  updateUserRole: (body, path) => api.updateUserRole(idIn(path), text(body, 'role') as Role),
  setUserPassword: (body, path) => api.setUserPassword(idIn(path), text(body, 'password')),
  selectDevice: (body) => api.selectDevice(textOrNull(body, 'deviceId')),
  updateSettings: (body) => api.updateSettings(body as SettingsPatch),
  testRecordingsDir: (body) => api.testRecordingsDir(textOrNull(body, 'path')),
  createBookmark: (body) =>
    api.createBookmark(number(body, 'timestampMs'), text(body, 'label'), textOrNull(body, 'note')),
  updateBookmark: (body, path) => api.updateBookmark(idIn(path), body as BookmarkPatch),
};

/** Each `api` method that sends a query string, called with the fixture's own values. */
const queryCalls: Record<string, (query: Record<string, string | number>) => unknown> = {
  peaks: (query) => api.peaks(number(query, 'fromMs'), number(query, 'toMs'), number(query, 'buckets')),
  sounds: (query) => api.sounds(number(query, 'fromMs'), number(query, 'toMs')),
  nextSound: (query) =>
    api.nextSound(number(query, 'fromMs'), text(query, 'direction') as 'forward' | 'backward'),
  exportPlan: (query) => api.exportPlan(number(query, 'fromMs'), number(query, 'toMs')),
  exportUrl: (query) => api.exportUrl(number(query, 'fromMs'), number(query, 'toMs')),
};

type FetchCall = { url: string; init: RequestInit | undefined };

let calls: FetchCall[] = [];

beforeEach(() => {
  calls = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      return new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } });
    }),
  );
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('client messages', () => {
  it('have exactly the variants and keys of the ClientMessage union', () => {
    const shapes = clientMessageShapes as unknown as Record<string, AnyShape>;
    const problems = sameNames(
      'client-messages.json',
      Object.keys(shapes),
      Object.keys(fixtures.clientMessages),
    );
    const coverage = new Coverage();
    for (const [name, fields] of Object.entries(shapes)) {
      for (const example of fixtures.clientMessages[name] ?? []) {
        expect(example.type).toBe(name);
        checkObject(example, fields, `client-messages.json ${name}`, coverage, problems);
      }
    }
    expect([...problems, ...coverage.gaps()]).toEqual([]);
  });

  it('show every player state the union has', () => {
    const states: Record<PlayerState, true> = { idle: true, playing: true, paused: true };
    const sent = (fixtures.clientMessages.player ?? []).map((example) => example.state);
    expect([...sent].sort()).toEqual(Object.keys(states).sort());
  });

  describe('through the stream socket', () => {
    class FakeWebSocket {
      static readonly OPEN = 1;
      static latest: FakeWebSocket | null = null;
      readyState = 0;
      binaryType = 'blob';
      sent: string[] = [];
      onopen: (() => void) | null = null;
      onmessage: (() => void) | null = null;
      onclose: (() => void) | null = null;
      onerror: (() => void) | null = null;

      constructor() {
        FakeWebSocket.latest = this;
      }

      send(data: string) {
        this.sent.push(data);
      }

      close() {}

      open() {
        this.readyState = FakeWebSocket.OPEN;
        this.onopen?.();
      }
    }

    function openSocket(): { socket: StreamSocket; fake: FakeWebSocket } {
      vi.stubGlobal('WebSocket', FakeWebSocket);
      vi.stubGlobal('window', {
        location: { protocol: 'http:', host: 'recorder.test:8080' },
        setTimeout: (callback: () => void, delay: number) => setTimeout(callback, delay),
        clearTimeout: (handle: number) => clearTimeout(handle),
        setInterval: (callback: () => void, delay: number) => setInterval(callback, delay),
        clearInterval: (handle: number) => clearInterval(handle),
      });
      const socket = new StreamSocket({
        onFrame: vi.fn(),
        onMessage: vi.fn(),
        onOpen: vi.fn(),
        onClose: vi.fn(),
      });
      socket.connect();
      const fake = FakeWebSocket.latest;
      if (!fake) {
        throw new Error('no socket was opened');
      }
      fake.open();
      return { socket, fake };
    }

    it('go out as the JSON in the fixture', () => {
      const { socket, fake } = openSocket();
      const all = Object.values(fixtures.clientMessages).flat();
      for (const example of all) {
        socket.send(example as ClientMessage);
      }
      expect(fake.sent.map((message) => JSON.parse(message) as unknown)).toEqual(all);
    });

    it('include the keep alive ping exactly as the fixture shows it', () => {
      const ping = fixtures.clientMessages.ping?.[0];
      if (!ping) {
        throw new Error('no ping fixture');
      }
      vi.useFakeTimers();
      vi.setSystemTime(number(ping, 'clientTimeMs') - 15_000);
      const { fake } = openSocket();
      vi.advanceTimersByTime(15_000);
      expect(fake.sent.map((message) => JSON.parse(message) as unknown)).toEqual([ping]);
    });
  });
});

describe('REST requests', () => {
  it('have a fixture for every api method that sends something, and none for any other', () => {
    const named = (kind: 'body' | 'query') =>
      Object.entries(payloads)
        .filter(([, payload]) => payload === kind)
        .map(([name]) => name);

    expect([
      ...sameNames('requests.json bodies', named('body'), Object.keys(fixtures.requests.bodies)),
      ...sameNames('requests.json queries', named('query'), Object.keys(fixtures.requests.queries)),
      ...sameNames('body calls in this test', named('body'), Object.keys(bodyCalls)),
      ...sameNames('query calls in this test', named('query'), Object.keys(queryCalls)),
    ]).toEqual([]);
  });

  it.each(Object.entries(fixtures.requests.bodies))(
    'api.%s sends the body, verb and path in the fixture',
    async (name, examples) => {
      const call = bodyCalls[name];
      expect(call, `no call for ${name}`).toBeDefined();
      for (const example of examples) {
        calls = [];
        await call(example.body, example.path);
        expect(calls).toHaveLength(1);
        expect(calls[0].url).toBe(`/api${example.path}`);
        expect(calls[0].init?.method).toBe(example.method);
        expect(JSON.parse(String(calls[0].init?.body)) as unknown).toEqual(example.body);
      }
    },
  );

  it.each(Object.entries(fixtures.requests.queries))(
    'api.%s sends the query and path in the fixture',
    async (name, examples) => {
      const call = queryCalls[name];
      expect(call, `no call for ${name}`).toBeDefined();
      for (const example of examples) {
        calls = [];
        const returned = await call(example.query);
        const url = typeof returned === 'string' ? returned : calls[0]?.url;
        const parsed = new URL(String(url), 'http://recorder.test:8080');
        expect(parsed.pathname).toBe(`/api${example.path}`);
        expect(Object.fromEntries(parsed.searchParams)).toEqual(
          Object.fromEntries(
            Object.entries(example.query).map(([key, value]) => [key, String(value)]),
          ),
        );
      }
    },
  );

  it('patch settings and bookmarks with exactly the keys of their TypeScript types', () => {
    const problems: string[] = [];
    const coverage = new Coverage();
    const partials: [string, AnyShape][] = [
      ['updateSettings', settingsPatchShape as unknown as AnyShape],
      ['updateBookmark', bookmarkPatchShape as unknown as AnyShape],
    ];
    for (const [name, fields] of partials) {
      const examples = fixtures.requests.bodies[name] ?? [];
      const sent = new Set<string>();
      for (const example of examples) {
        checkPartial(example.body, fields, `requests.json ${name}`, coverage, problems);
        Object.keys(example.body).forEach((key) => sent.add(key));
      }
      problems.push(...sameNames(`requests.json ${name} keys`, Object.keys(fields), [...sent]));
    }
    expect([...problems, ...coverage.gaps()]).toEqual([]);
  });
});
