/**
 * Update notices as the service reports them, for the banner, the what's new dialog and the Updates card.
 * Each builder gives a complete, realistic answer so a test only overrides what its behaviour hinges on.
 */

import type { InstallInfo, ReleaseInfo, UpdateStatus } from '@/api/types';

export function release(version: string, overrides: Partial<ReleaseInfo> = {}): ReleaseInfo {
  return {
    version,
    tag: `v${version}`,
    prerelease: version.includes('beta'),
    publishedAtMs: Date.UTC(2026, 8, 20, 12),
    notes: `### Added\n\n- Something new in ${version} [OAR-1]`,
    url: `https://github.com/shibbirweb/on-air-record/releases/tag/v${version}`,
    ...overrides,
  };
}

export const INSTALLER: InstallInfo = {
  kind: 'installer',
  dir: '/home/radio/on-air-record',
  os: 'linux',
  target: 'x86_64-unknown-linux-gnu',
};

/** The service's answer when 0.9.0 is out and 0.8.1 is running. */
export function updateStatus(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  const newest = release('0.9.0');
  return {
    currentVersion: '0.8.1',
    channel: 'stable',
    automatic: true,
    checkedAtMs: Date.UTC(2026, 8, 27, 9, 30),
    error: null,
    available: newest,
    releases: [newest],
    install: INSTALLER,
    releasesUrl: 'https://github.com/shibbirweb/on-air-record/releases',
    ...overrides,
  };
}

/** The service's answer when nothing newer is out. */
export function upToDate(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return updateStatus({ available: null, releases: [], ...overrides });
}
