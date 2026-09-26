import { afterEach, describe, expect, it, vi } from 'vitest';

import type { UpdateStatus } from '@/api/types';

import { shouldShowBanner, useUpdateStore } from '../useUpdateStore';

function status(available: string | null): UpdateStatus {
  const release = available
    ? { version: available, tag: `v${available}`, prerelease: false, publishedAtMs: 0, notes: '', url: '' }
    : null;
  return {
    currentVersion: '0.6.0',
    channel: 'stable',
    automatic: true,
    checkedAtMs: 0,
    error: null,
    available: release,
    releases: release ? [release] : [],
    install: { kind: 'manual', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' },
    releasesUrl: '',
  };
}

describe('shouldShowBanner', () => {
  it('shows when an update exists that was not dismissed here', () => {
    expect(shouldShowBanner(status('0.7.0'), null)).toBe(true);
    expect(shouldShowBanner(status('0.7.0'), '0.6.1')).toBe(true);
  });

  it('stays hidden when up to date, not yet checked, or dismissed for that version', () => {
    expect(shouldShowBanner(status(null), null)).toBe(false);
    expect(shouldShowBanner(null, null)).toBe(false);
    expect(shouldShowBanner(status('0.7.0'), '0.7.0')).toBe(false);
  });
});

describe('dismiss', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    useUpdateStore.setState({ dismissed: null });
  });

  it('remembers the version in this browser', () => {
    const stored = new Map<string, string>();
    vi.stubGlobal('window', {
      localStorage: {
        getItem: (key: string) => stored.get(key) ?? null,
        setItem: (key: string, value: string) => stored.set(key, value),
      },
    });
    useUpdateStore.getState().dismiss('0.7.0');
    expect(useUpdateStore.getState().dismissed).toBe('0.7.0');
    expect(stored.get('oar.updates.dismissed')).toBe('0.7.0');
  });

  it('still hides the banner for this page when storage is unavailable', () => {
    vi.stubGlobal('window', {
      localStorage: {
        setItem: () => {
          throw new Error('blocked');
        },
      },
    });
    expect(() => useUpdateStore.getState().dismiss('0.7.0')).not.toThrow();
    expect(useUpdateStore.getState().dismissed).toBe('0.7.0');
  });
});
