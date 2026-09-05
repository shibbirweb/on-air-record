/**
 * REST transport.
 *
 * Every path is relative, because the UI is served by the same process that serves the API in production
 * and Vite proxies the same paths in development. That removes the class of bugs where a base URL is right
 * on one machine and wrong on another.
 */

import type {
  Health,
  InputDevice,
  Peaks,
  RecordingDay,
  RecordingSession,
  ServiceStatus,
  Settings,
  SettingsPatch,
  Storage,
  TimelineRange,
} from './types';

const API_BASE = '/api';

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

    throw new ApiError(message, code, response.status);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return (await response.json()) as T;
}

export const api = {
  health: () => request<Health>('/health'),

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

  timelineRange: () => request<TimelineRange>('/timeline/range'),

  recordingDays: () =>
    request<{ days: RecordingDay[] }>('/timeline/days').then((body) => body.days),

  peaks: (fromMs: number, toMs: number, buckets: number) =>
    request<Peaks>(
      `/timeline/peaks?fromMs=${Math.round(fromMs)}&toMs=${Math.round(toMs)}&buckets=${buckets}`,
    ),

  sessions: () =>
    request<{ sessions: RecordingSession[] }>('/sessions').then((body) => body.sessions),

  storage: () => request<Storage>('/storage'),
};
