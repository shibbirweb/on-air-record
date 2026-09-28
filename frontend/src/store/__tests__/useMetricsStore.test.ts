/**
 * The scrape token as the Monitoring card sees it: when it was made, and the new token for as long as the
 * card is open. The server sends a token exactly once, when it is made, so the store is the only place it
 * ever exists in the page, and it must be forgotten when the admin moves on.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { api, ApiError } from '@/api/client';

import { useMetricsStore } from '../useMetricsStore';

const initial = useMetricsStore.getState();

beforeEach(() => {
  useMetricsStore.setState(initial, true);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('refresh', () => {
  it('learns whether a token exists and when it was made', async () => {
    vi.spyOn(api, 'metricsToken').mockResolvedValue({ createdAtMs: 1_000 });
    await useMetricsStore.getState().refresh();
    expect(useMetricsStore.getState()).toMatchObject({ createdAtMs: 1_000, loaded: true, error: null });
  });

  it('knows when there is none', async () => {
    vi.spyOn(api, 'metricsToken').mockResolvedValue({ createdAtMs: null });
    await useMetricsStore.getState().refresh();
    expect(useMetricsStore.getState()).toMatchObject({ createdAtMs: null, loaded: true });
  });

  it('says what went wrong', async () => {
    vi.spyOn(api, 'metricsToken').mockRejectedValue(new ApiError('database is locked', 'internal', 500));
    await useMetricsStore.getState().refresh();
    expect(useMetricsStore.getState().error).toBe('database is locked');
  });
});

describe('create', () => {
  it('holds the new token to show once, with its date', async () => {
    vi.spyOn(api, 'createMetricsToken').mockResolvedValue({ token: 'abc', createdAtMs: 2_000 });
    await useMetricsStore.getState().create();
    expect(useMetricsStore.getState()).toMatchObject({
      freshToken: 'abc',
      createdAtMs: 2_000,
      busy: false,
      error: null,
    });
  });

  it('is busy while the request is out', async () => {
    let finish: (value: { token: string; createdAtMs: number }) => void = () => undefined;
    vi.spyOn(api, 'createMetricsToken').mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const pending = useMetricsStore.getState().create();
    expect(useMetricsStore.getState().busy).toBe(true);
    finish({ token: 'abc', createdAtMs: 1 });
    await pending;
    expect(useMetricsStore.getState().busy).toBe(false);
  });

  it('keeps the old state and says why when it fails', async () => {
    useMetricsStore.setState({ createdAtMs: 1_000 });
    vi.spyOn(api, 'createMetricsToken').mockRejectedValue(new Error('offline'));
    await useMetricsStore.getState().create();
    expect(useMetricsStore.getState()).toMatchObject({
      createdAtMs: 1_000,
      freshToken: null,
      busy: false,
      error: 'could not reach the recorder',
    });
  });
});

describe('revoke', () => {
  it('forgets the token and its date', async () => {
    useMetricsStore.setState({ createdAtMs: 1_000, freshToken: 'abc' });
    vi.spyOn(api, 'revokeMetricsToken').mockResolvedValue({ createdAtMs: null });
    await useMetricsStore.getState().revoke();
    expect(useMetricsStore.getState()).toMatchObject({ createdAtMs: null, freshToken: null, busy: false });
  });

  it('keeps the token and says why when it fails', async () => {
    useMetricsStore.setState({ createdAtMs: 1_000 });
    vi.spyOn(api, 'revokeMetricsToken').mockRejectedValue(new ApiError('admins only', 'forbidden', 403));
    await useMetricsStore.getState().revoke();
    expect(useMetricsStore.getState()).toMatchObject({ createdAtMs: 1_000, error: 'admins only' });
  });
});

describe('forgetFreshToken', () => {
  it('drops the token shown once but keeps the date', () => {
    useMetricsStore.setState({ createdAtMs: 1_000, freshToken: 'abc' });
    useMetricsStore.getState().forgetFreshToken();
    expect(useMetricsStore.getState()).toMatchObject({ createdAtMs: 1_000, freshToken: null });
  });
});
