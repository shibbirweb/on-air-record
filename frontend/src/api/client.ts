/**
 * REST transport.
 *
 * Every path is relative, because the UI is served by the same process that serves the API in production
 * and Vite proxies the same paths in development. That removes the class of bugs where a base URL is right
 * on one machine and wrong on another.
 */

import type {
  AuthState,
  Bookmark,
  DirectoryTest,
  ExportPlan,
  Health,
  InputDevice,
  Peaks,
  RecordingDay,
  RecordingSession,
  ServiceStatus,
  Settings,
  SettingsPatch,
  Role,
  Storage,
  TimelineRange,
  User,
} from './types';

const API_BASE = '/api';

let onUnauthorized: (() => void) | null = null;

/**
 * Hear about a session that has ended, from whichever request notices first.
 *
 * A session can end between two requests: signed out in another tab, account removed, password changed
 * elsewhere. Routing every 401 here lets the auth store show the login page without every other store
 * having to know that logins exist.
 */
export function setUnauthorizedHandler(handler: (() => void) | null): void {
  onUnauthorized = handler;
}

/** An error the server described in its own words, so the UI can show something specific. */
export class ApiError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(message: string, code: string, status: number) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
    this.status = status;
  }
}

type ErrorEnvelope = {
  error?: {
    code?: string;
    message?: string;
  };
};

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;

  try {
    response = await fetch(`${API_BASE}${path}`, {
      headers: init?.body ? { 'content-type': 'application/json' } : undefined,
      ...init,
    });
  } catch (cause) {
    // A network failure is the normal state while the service restarts, so it gets the same shape as a
    // server error rather than an unhandled rejection somewhere up the tree.
    throw new ApiError(
      cause instanceof Error ? cause.message : 'the service is unreachable',
      'network',
      0,
    );
  }

  if (!response.ok) {
    let code = 'internal';
    let message = `${response.status} ${response.statusText}`;

    try {
      const envelope = (await response.json()) as ErrorEnvelope;
      code = envelope.error?.code ?? code;
      message = envelope.error?.message ?? message;
    } catch {
      // The body was not the documented envelope. The status line is still worth reporting.
    }

    // A failed login is also a 401, but it is the answer to the form, not news that a session ended.
    if (response.status === 401 && !path.startsWith('/auth/')) {
      onUnauthorized?.();
    }

    throw new ApiError(message, code, response.status);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return (await response.json()) as T;
}

export const api = {
  health: () => request<Health>('/health'),

  authState: () => request<AuthState>('/auth/state'),

  chooseOpen: () => request<AuthState>('/auth/open', { method: 'POST' }),

  setUp: (email: string, password: string) =>
    request<AuthState>('/auth/setup', {
      method: 'POST',
      body: JSON.stringify({ email, password }),
    }),

  logIn: (email: string, password: string) =>
    request<AuthState>('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ email, password }),
    }),

  logOut: () => request<AuthState>('/auth/logout', { method: 'POST' }),

  changePassword: (currentPassword: string, newPassword: string) =>
    request<void>('/auth/password', {
      method: 'POST',
      body: JSON.stringify({ currentPassword, newPassword }),
    }),

  users: () => request<{ users: User[] }>('/users').then((body) => body.users),

  createUser: (email: string, password: string, role: Role) =>
    request<User>('/users', {
      method: 'POST',
      body: JSON.stringify({ email, password, role }),
    }),

  updateUserRole: (userId: number, role: Role) =>
    request<User>(`/users/${userId}`, {
      method: 'PATCH',
      body: JSON.stringify({ role }),
    }),

  setUserPassword: (userId: number, password: string) =>
    request<void>(`/users/${userId}/password`, {
      method: 'POST',
      body: JSON.stringify({ password }),
    }),

  deleteUser: (userId: number) => request<void>(`/users/${userId}`, { method: 'DELETE' }),

  status: () => request<ServiceStatus>('/status'),

  startCapture: () => request<ServiceStatus>('/capture/start', { method: 'POST' }),

  stopCapture: () => request<ServiceStatus>('/capture/stop', { method: 'POST' }),

  devices: () => request<{ devices: InputDevice[] }>('/devices').then((body) => body.devices),

  selectDevice: (deviceId: string | null) =>
    request<ServiceStatus>('/devices/select', {
      method: 'POST',
      body: JSON.stringify({ deviceId }),
    }),

  settings: () => request<Settings>('/settings'),

  updateSettings: (patch: SettingsPatch) =>
    request<Settings>('/settings', {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),

  settingsDefaults: () => request<Settings>('/settings/defaults'),

  testRecordingsDir: (path: string | null) =>
    request<DirectoryTest>('/settings/test-recordings-dir', {
      method: 'POST',
      body: JSON.stringify({ path }),
    }),

  resetSettings: () => request<Settings>('/settings/reset', { method: 'POST' }),

  timelineRange: () => request<TimelineRange>('/timeline/range'),

  recordingDays: () =>
    request<{ days: RecordingDay[] }>('/timeline/days').then((body) => body.days),

  peaks: (fromMs: number, toMs: number, buckets: number) =>
    request<Peaks>(
      `/timeline/peaks?fromMs=${Math.round(fromMs)}&toMs=${Math.round(toMs)}&buckets=${buckets}`,
    ),

  bookmarks: () =>
    request<{ bookmarks: Bookmark[] }>('/bookmarks').then((body) => body.bookmarks),

  createBookmark: (timestampMs: number, label: string, note?: string | null) =>
    request<Bookmark>('/bookmarks', {
      method: 'POST',
      body: JSON.stringify({ timestampMs: Math.round(timestampMs), label, note: note ?? null }),
    }),

  updateBookmark: (id: number, patch: { label?: string; note?: string | null }) =>
    request<Bookmark>(`/bookmarks/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),

  deleteBookmark: (id: number) =>
    request<{ bookmarks: Bookmark[] }>(`/bookmarks/${id}`, { method: 'DELETE' }).then(
      (body) => body.bookmarks,
    ),

  sessions: () =>
    request<{ sessions: RecordingSession[] }>('/sessions').then((body) => body.sessions),

  storage: () => request<Storage>('/storage'),

  exportPlan: (fromMs: number, toMs: number) =>
    request<ExportPlan>(
      `/export/plan?fromMs=${Math.round(fromMs)}&toMs=${Math.round(toMs)}`,
    ),

  /**
   * The download URL, not the bytes.
   *
   * Fetching a gigabyte into memory to hand it back as a blob would defeat the streaming the server
   * does; letting the browser follow the link keeps it a normal download with a progress bar, and
   * `Content-Disposition` supplies the filename.
   */
  exportUrl: (fromMs: number, toMs: number) =>
    `${API_BASE}/export?fromMs=${Math.round(fromMs)}&toMs=${Math.round(toMs)}`,
};
