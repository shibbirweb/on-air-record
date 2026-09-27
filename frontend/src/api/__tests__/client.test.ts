/**
 * The REST client: the one `request` helper every call goes through, and the table of methods on `api`.
 *
 * The helper is where a failure of any kind becomes an `ApiError` the UI can show, and where a 401 is
 * routed to the handler that sends the page back to sign in, so each of those paths is tested on its own.
 * The methods are thin, and what goes wrong in them is a wrong path, verb or body, which only shows when
 * the server answers 404 or 400. A table over every method checks all three against a stubbed `fetch`.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { api, ApiError, setUnauthorizedHandler } from '../client';

type FetchCall = { url: string; init: RequestInit | undefined };

let calls: FetchCall[] = [];
let respond: () => Response | Promise<Response> = () => json({});

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

beforeEach(() => {
  calls = [];
  respond = () => json({});
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      return respond();
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  setUnauthorizedHandler(null);
});

describe('request', () => {
  it('returns the parsed JSON body of a successful answer', async () => {
    respond = () => json({ status: 'ok', version: '0.8.1', uptimeMs: 5 });
    await expect(api.health()).resolves.toEqual({ status: 'ok', version: '0.8.1', uptimeMs: 5 });
  });

  it('returns nothing for 204 No Content without trying to parse a body', async () => {
    respond = () => new Response(null, { status: 204 });
    await expect(api.deleteUser(3)).resolves.toBeUndefined();
  });

  it('sends a JSON content type only when there is a body', async () => {
    await api.health();
    await api.logIn('owner@example.com', 'a long password');

    expect(calls[0].init?.headers).toBeUndefined();
    expect(calls[1].init?.headers).toEqual({ 'content-type': 'application/json' });
  });

  it('prefixes every path with /api, relative to the page', async () => {
    await api.status();
    expect(calls[0].url).toBe('/api/status');
  });

  it('turns the documented error envelope into an ApiError with its code, message and status', async () => {
    respond = () => json({ error: { code: 'conflict', message: 'that email is taken' } }, 409);

    const error = await api.createUser('a@b.c', 'password1', 'listener').catch((cause) => cause);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({
      name: 'ApiError',
      code: 'conflict',
      message: 'that email is taken',
      status: 409,
    });
  });

  it('fills in whatever part of the envelope is missing', async () => {
    respond = () => json({ error: { message: 'only a message' } }, 400);
    await expect(api.status()).rejects.toMatchObject({ code: 'internal', message: 'only a message' });

    respond = () =>
      new Response(JSON.stringify({ error: { code: 'bad_request' } }), {
        status: 400,
        statusText: 'Bad Request',
      });
    await expect(api.status()).rejects.toMatchObject({
      code: 'bad_request',
      message: '400 Bad Request',
    });
  });

  it('reports the status line when the error body is not the envelope', async () => {
    respond = () => new Response('<html>Bad Gateway</html>', { status: 502, statusText: 'Bad Gateway' });

    await expect(api.status()).rejects.toMatchObject({
      code: 'internal',
      message: '502 Bad Gateway',
      status: 502,
    });
  });

  it('gives a network failure the same shape as a server error, with status 0', async () => {
    respond = () => {
      throw new TypeError('Failed to fetch');
    };

    const error = await api.status().catch((cause) => cause);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ code: 'network', status: 0, message: 'Failed to fetch' });
  });

  it('describes a network failure that threw something other than an Error', async () => {
    respond = () => {
      throw 'offline';
    };
    await expect(api.status()).rejects.toMatchObject({
      code: 'network',
      message: 'the service is unreachable',
    });
  });
});

describe('the unauthorized handler', () => {
  it('is told when any ordinary request comes back 401', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    respond = () => json({ error: { code: 'unauthorized', message: 'sign in first' } }, 401);

    await expect(api.status()).rejects.toMatchObject({ status: 401 });
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('is told when the account two factor routes come back 401, since the session is gone', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    respond = () => json({}, 401);

    await api.twoFactorStatus().catch(() => undefined);
    await api.enableTwoFactor('123456').catch(() => undefined);
    expect(handler).toHaveBeenCalledTimes(2);
  });

  it('is not told about a wrong password or code, which is the answer to the form', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    respond = () => json({ error: { code: 'unauthorized', message: 'wrong password' } }, 401);

    await expect(api.logIn('a@b.c', 'wrong')).rejects.toMatchObject({ message: 'wrong password' });
    await expect(api.verifyLogin('000000')).rejects.toMatchObject({ status: 401 });
    await expect(api.setUp('a@b.c', 'password1')).rejects.toMatchObject({ status: 401 });
    expect(handler).not.toHaveBeenCalled();
  });

  it('is not told about errors other than 401', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    for (const status of [400, 403, 404, 500]) {
      respond = () => json({}, status);
      await api.status().catch(() => undefined);
    }
    expect(handler).not.toHaveBeenCalled();
  });

  it('still rejects with the ApiError when no handler is registered', async () => {
    respond = () => json({}, 401);
    await expect(api.status()).rejects.toBeInstanceOf(ApiError);
  });

  it('stops being told once it is unregistered', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    setUnauthorizedHandler(null);
    respond = () => json({}, 401);

    await api.status().catch(() => undefined);
    expect(handler).not.toHaveBeenCalled();
  });
});

type MethodCase = {
  name: string;
  call: () => Promise<unknown>;
  path: string;
  method: string;
  body?: unknown;
  /** What the server answers, when the method unwraps or depends on it. */
  answer?: unknown;
  /** What the method resolves to, when it is not the answer itself. */
  result?: unknown;
};

const USER = { id: 7, email: 'a@b.c', role: 'listener', createdAtMs: 0, twoFactorEnabled: false };

const METHODS: MethodCase[] = [
  { name: 'health', call: () => api.health(), path: '/health', method: 'GET' },
  { name: 'authState', call: () => api.authState(), path: '/auth/state', method: 'GET' },
  { name: 'chooseOpen', call: () => api.chooseOpen(), path: '/auth/open', method: 'POST' },
  {
    name: 'setUp',
    call: () => api.setUp('owner@example.com', 'secret pass'),
    path: '/auth/setup',
    method: 'POST',
    body: { email: 'owner@example.com', password: 'secret pass' },
  },
  {
    name: 'logIn',
    call: () => api.logIn('owner@example.com', 'secret pass'),
    path: '/auth/login',
    method: 'POST',
    body: { email: 'owner@example.com', password: 'secret pass' },
  },
  {
    name: 'verifyLogin',
    call: () => api.verifyLogin('123456'),
    path: '/auth/login/verify',
    method: 'POST',
    body: { code: '123456' },
  },
  { name: 'logOut', call: () => api.logOut(), path: '/auth/logout', method: 'POST' },
  { name: 'twoFactorStatus', call: () => api.twoFactorStatus(), path: '/auth/two-factor', method: 'GET' },
  {
    name: 'beginTwoFactorSetup',
    call: () => api.beginTwoFactorSetup(),
    path: '/auth/two-factor/setup',
    method: 'POST',
  },
  {
    name: 'enableTwoFactor',
    call: () => api.enableTwoFactor('654321'),
    path: '/auth/two-factor/enable',
    method: 'POST',
    body: { code: '654321' },
    answer: { recoveryCodes: ['aaaaa-bbbbb', 'ccccc-ddddd'] },
    result: ['aaaaa-bbbbb', 'ccccc-ddddd'],
  },
  {
    name: 'disableTwoFactor',
    call: () => api.disableTwoFactor('secret pass'),
    path: '/auth/two-factor/disable',
    method: 'POST',
    body: { password: 'secret pass' },
  },
  {
    name: 'regenerateRecoveryCodes',
    call: () => api.regenerateRecoveryCodes('secret pass'),
    path: '/auth/two-factor/recovery-codes',
    method: 'POST',
    body: { password: 'secret pass' },
    answer: { recoveryCodes: ['eeeee-fffff'] },
    result: ['eeeee-fffff'],
  },
  {
    name: 'resetUserTwoFactor',
    call: () => api.resetUserTwoFactor(7),
    path: '/users/7/two-factor',
    method: 'DELETE',
  },
  {
    name: 'changePassword',
    call: () => api.changePassword('old pass', 'new pass'),
    path: '/auth/password',
    method: 'POST',
    body: { currentPassword: 'old pass', newPassword: 'new pass' },
  },
  {
    name: 'users',
    call: () => api.users(),
    path: '/users',
    method: 'GET',
    answer: { users: [USER] },
    result: [USER],
  },
  {
    name: 'createUser',
    call: () => api.createUser('a@b.c', 'secret pass', 'admin'),
    path: '/users',
    method: 'POST',
    body: { email: 'a@b.c', password: 'secret pass', role: 'admin' },
  },
  {
    name: 'updateUserRole',
    call: () => api.updateUserRole(7, 'admin'),
    path: '/users/7',
    method: 'PATCH',
    body: { role: 'admin' },
  },
  {
    name: 'setUserPassword',
    call: () => api.setUserPassword(7, 'secret pass'),
    path: '/users/7/password',
    method: 'POST',
    body: { password: 'secret pass' },
  },
  { name: 'deleteUser', call: () => api.deleteUser(7), path: '/users/7', method: 'DELETE' },
  { name: 'status', call: () => api.status(), path: '/status', method: 'GET' },
  { name: 'startCapture', call: () => api.startCapture(), path: '/capture/start', method: 'POST' },
  { name: 'stopCapture', call: () => api.stopCapture(), path: '/capture/stop', method: 'POST' },
  {
    name: 'devices',
    call: () => api.devices(),
    path: '/devices',
    method: 'GET',
    answer: { devices: [{ id: 'mic' }] },
    result: [{ id: 'mic' }],
  },
  {
    name: 'selectDevice',
    call: () => api.selectDevice('usb-mic'),
    path: '/devices/select',
    method: 'POST',
    body: { deviceId: 'usb-mic' },
  },
  {
    name: 'selectDevice with the system default',
    call: () => api.selectDevice(null),
    path: '/devices/select',
    method: 'POST',
    body: { deviceId: null },
  },
  { name: 'settings', call: () => api.settings(), path: '/settings', method: 'GET' },
  {
    name: 'updateSettings',
    call: () => api.updateSettings({ gain: 2, retentionHours: null }),
    path: '/settings',
    method: 'PATCH',
    body: { gain: 2, retentionHours: null },
  },
  {
    name: 'settingsDefaults',
    call: () => api.settingsDefaults(),
    path: '/settings/defaults',
    method: 'GET',
  },
  { name: 'updates', call: () => api.updates(), path: '/updates', method: 'GET' },
  { name: 'checkForUpdates', call: () => api.checkForUpdates(), path: '/updates/check', method: 'POST' },
  {
    name: 'testRecordingsDir',
    call: () => api.testRecordingsDir('/mnt/audio'),
    path: '/settings/test-recordings-dir',
    method: 'POST',
    body: { path: '/mnt/audio' },
  },
  {
    name: 'testRecordingsDir with the default location',
    call: () => api.testRecordingsDir(null),
    path: '/settings/test-recordings-dir',
    method: 'POST',
    body: { path: null },
  },
  { name: 'resetSettings', call: () => api.resetSettings(), path: '/settings/reset', method: 'POST' },
  { name: 'timelineRange', call: () => api.timelineRange(), path: '/timeline/range', method: 'GET' },
  {
    name: 'recordingDays',
    call: () => api.recordingDays(),
    path: '/timeline/days',
    method: 'GET',
    answer: { days: [{ day: '2026-09-05' }] },
    result: [{ day: '2026-09-05' }],
  },
  {
    name: 'peaks, with fractional bounds rounded',
    call: () => api.peaks(1000.4, 2000.6, 1200),
    path: '/timeline/peaks?fromMs=1000&toMs=2001&buckets=1200',
    method: 'GET',
  },
  {
    name: 'sounds, with fractional bounds rounded',
    call: () => api.sounds(1000.5, 1999.2),
    path: '/timeline/sounds?fromMs=1001&toMs=1999',
    method: 'GET',
  },
  {
    name: 'nextSound',
    call: () => api.nextSound(5000.7, 'backward'),
    path: '/timeline/sounds/next?fromMs=5001&direction=backward',
    method: 'GET',
    answer: { sound: { startMs: 1, endMs: 2, seekMs: 0, peak: 9 } },
    result: { startMs: 1, endMs: 2, seekMs: 0, peak: 9 },
  },
  {
    name: 'nextSound when there is none that way',
    call: () => api.nextSound(5000, 'forward'),
    path: '/timeline/sounds/next?fromMs=5000&direction=forward',
    method: 'GET',
    answer: { sound: null },
    result: null,
  },
  {
    name: 'bookmarks',
    call: () => api.bookmarks(),
    path: '/bookmarks',
    method: 'GET',
    answer: { bookmarks: [{ id: 1 }] },
    result: [{ id: 1 }],
  },
  {
    name: 'createBookmark, with the moment rounded and a missing note sent as null',
    call: () => api.createBookmark(1234.6, 'Intro'),
    path: '/bookmarks',
    method: 'POST',
    body: { timestampMs: 1235, label: 'Intro', note: null },
  },
  {
    name: 'createBookmark with a note',
    call: () => api.createBookmark(10, 'Intro', 'first words'),
    path: '/bookmarks',
    method: 'POST',
    body: { timestampMs: 10, label: 'Intro', note: 'first words' },
  },
  {
    name: 'updateBookmark',
    call: () => api.updateBookmark(4, { label: 'Renamed' }),
    path: '/bookmarks/4',
    method: 'PATCH',
    body: { label: 'Renamed' },
  },
  {
    name: 'deleteBookmark, which hands back what remains',
    call: () => api.deleteBookmark(4),
    path: '/bookmarks/4',
    method: 'DELETE',
    answer: { bookmarks: [{ id: 5 }] },
    result: [{ id: 5 }],
  },
  {
    name: 'sessions',
    call: () => api.sessions(),
    path: '/sessions',
    method: 'GET',
    answer: { sessions: [{ id: 2 }] },
    result: [{ id: 2 }],
  },
  { name: 'storage', call: () => api.storage(), path: '/storage', method: 'GET' },
  {
    name: 'exportPlan, with fractional bounds rounded',
    call: () => api.exportPlan(10.2, 20.9),
    path: '/export/plan?fromMs=10&toMs=21',
    method: 'GET',
  },
];

describe('api methods', () => {
  it.each(METHODS)('$name calls the right path, verb and body', async (entry) => {
    const answer = entry.answer ?? { echoed: entry.name };
    respond = () => json(answer);

    const result = await entry.call();

    expect(calls).toHaveLength(1);
    const [{ url, init }] = calls;
    expect(url).toBe(`/api${entry.path}`);
    expect(init?.method ?? 'GET').toBe(entry.method);
    if (entry.body === undefined) {
      expect(init?.body).toBeUndefined();
    } else {
      expect(JSON.parse(String(init?.body))).toEqual(entry.body);
    }
    expect(result).toEqual('result' in entry ? entry.result : answer);
  });

  it('covers every method on api apart from exportUrl, which makes no request', () => {
    const tested = new Set(METHODS.map((entry) => entry.name.split(/[ ,]/)[0]));
    const methods = Object.keys(api).filter((name) => name !== 'exportUrl');
    expect(methods.filter((name) => !tested.has(name))).toEqual([]);
  });

  it('builds the export download link without fetching anything', () => {
    expect(api.exportUrl(10.4, 20.5)).toBe('/api/export?fromMs=10&toMs=21');
    expect(calls).toEqual([]);
  });
});
