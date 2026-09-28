/**
 * Shared values for the settings page tests: a stored settings object that matches the server's defaults,
 * and builders for the status, storage and account shapes the cards read. Kept here so every test states
 * only the fields its behaviour depends on.
 */

import type { Role, ServiceStatus, Settings, Storage, User } from '@/api/types';

/** What the server reports on a fresh install, which is also what its defaults endpoint returns. */
export const STORED: Settings = {
  inputDeviceId: null,
  gain: 1,
  segmentSeconds: 10,
  retentionHours: 24,
  autoStart: true,
  autoStartDelaySeconds: 0,
  frameMs: 100,
  recordingSampleRate: null,
  recordingsDir: null,
  effectiveRecordingsDir: '/srv/oar/recordings',
  checkForUpdates: true,
  soundSensitivity: 'medium',
  activityRetentionDays: 90,
};

/** A service status whose capture runs at `sampleRate`, recording or not. */
export function serviceStatus(
  sampleRate: number,
  state: ServiceStatus['capture']['state'] = 'recording',
  deviceSampleRate: number = sampleRate,
): ServiceStatus {
  return {
    capture: {
      state,
      sessionId: state === 'recording' ? 7 : null,
      deviceId: 'mic',
      deviceName: 'USB microphone',
      sampleRate,
      deviceSampleRate,
      channels: 1,
      frameMs: 100,
      startedAtMs: state === 'recording' ? 1_757_000_000_000 : null,
      droppedFrames: 0,
      error: null,
    },
    levels: { rms: 0, peak: 0 },
    listeners: 0,
    serverTimeMs: 1_757_000_000_000,
    liveEdgeMs: null,
  };
}

/** Disk usage as the storage endpoint reports it. */
export function storageUsage(bytes: number, bytesPerHour: number): Storage {
  return {
    bytes,
    segmentCount: 12,
    oldestMs: null,
    newestMs: null,
    retentionHours: 24,
    dataDir: '/srv/oar',
    recordingsDir: '/srv/oar/recordings',
    bytesPerHour,
    projectedMaxBytes: bytesPerHour * 24,
  };
}

export function account(id: number, role: Role, twoFactorEnabled = false): User {
  return { id, email: `person${id}@example.com`, role, createdAtMs: 0, twoFactorEnabled };
}
