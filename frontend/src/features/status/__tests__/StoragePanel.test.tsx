// @vitest-environment jsdom

/**
 * The storage panel: how much the recordings take, how far back they reach, the retention window, the
 * oldest moment still playable, the recent sessions and the data directory, and what it shows before the
 * first answer and with nothing recorded. The store is set directly; fetching is the store's.
 */

import '@/test/dom';

import { render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import type { RecordingSession, Storage } from '@/api/types';
import { formatBytes, formatDateTime } from '@/lib/format';
import { useStorageStore } from '@/store/useStorageStore';

import { StoragePanel } from '../StoragePanel';

const NOW = new Date(2026, 8, 16, 15, 0, 0).getTime();

const STORAGE: Storage = {
  bytes: 5_368_709_120,
  segmentCount: 4_320,
  oldestMs: NOW - (26 * 3_600_000 + 30 * 60_000),
  newestMs: NOW,
  retentionHours: 72,
  dataDir: '/var/lib/on-air-record',
  recordingsDir: '/var/lib/on-air-record/recordings',
  bytesPerHour: 345_600_000,
  projectedMaxBytes: 24_883_200_000,
};

const session = (id: number, hoursAgo: number): RecordingSession => ({
  id,
  deviceId: `device-${id}`,
  deviceName: `Microphone ${id}`,
  sampleRate: 48_000,
  channels: 1,
  startedAtMs: NOW - hoursAgo * 3_600_000,
  endedAtMs: null,
  segmentCount: 10,
  bytes: id * 1_048_576,
});

const pristine = useStorageStore.getState();

/** The value next to a label in the details list. */
const detail = (label: string) => screen.getByText(label).nextElementSibling;

beforeEach(() => {
  useStorageStore.setState(pristine, true);
  useStorageStore.setState({ storage: STORAGE, sessions: [session(1, 2)] });
});

describe('the storage figures', () => {
  it('give the space on disk and the number of segments', () => {
    render(<StoragePanel />);
    expect(detail('On disk')).toHaveTextContent('5.0 GB');
    expect(detail('Segments')).toHaveTextContent('4320');
  });

  it('give how far back the recordings actually reach', () => {
    render(<StoragePanel />);
    expect(detail('History')).toHaveTextContent('26:30:00');
  });

  it('give the retention window in hours', () => {
    render(<StoragePanel />);
    expect(detail('Retention')).toHaveTextContent('72 h');
  });

  it('say recordings are kept forever when there is no retention window', () => {
    useStorageStore.setState({ storage: { ...STORAGE, retentionHours: null } });
    render(<StoragePanel />);
    expect(detail('Retention')).toHaveTextContent('Forever');
  });

  it('give the oldest moment still playable', () => {
    render(<StoragePanel />);
    expect(detail('Oldest')).toHaveTextContent(formatDateTime(STORAGE.oldestMs));
  });

  it('show the data directory in full on pointing', () => {
    render(<StoragePanel />);
    expect(screen.getByText('/var/lib/on-air-record')).toHaveAttribute('title', '/var/lib/on-air-record');
  });

  it('show no history and an unknown oldest moment when nothing has been recorded', () => {
    useStorageStore.setState({
      storage: { ...STORAGE, bytes: 0, segmentCount: 0, oldestMs: null, newestMs: null },
      sessions: [],
    });
    render(<StoragePanel />);
    expect(detail('On disk')).toHaveTextContent('0 B');
    expect(detail('Segments')).toHaveTextContent('0');
    expect(detail('History')).toHaveTextContent('00:00:00');
    expect(detail('Oldest')).toHaveTextContent('unknown');
  });

  it('show zeros and no directory before the first answer arrives', () => {
    useStorageStore.setState({ storage: null, sessions: [] });
    render(<StoragePanel />);
    expect(detail('On disk')).toHaveTextContent('0 B');
    expect(detail('Segments')).toHaveTextContent('0');
    expect(detail('History')).toHaveTextContent('00:00:00');
    expect(detail('Oldest')).toHaveTextContent('unknown');
    expect(screen.queryByText('/var/lib/on-air-record')).not.toBeInTheDocument();
  });
});

describe('the recent sessions', () => {
  it('list each session by microphone, start time and size', () => {
    render(<StoragePanel />);
    const item = screen.getByRole('listitem');
    expect(within(item).getByText('Microphone 1')).toBeInTheDocument();
    expect(item).toHaveTextContent(`${formatDateTime(NOW - 2 * 3_600_000)} \u00b7 ${formatBytes(1_048_576)}`);
  });

  it('show at most the five most recent', () => {
    useStorageStore.setState({
      sessions: [session(1, 1), session(2, 2), session(3, 3), session(4, 4), session(5, 5), session(6, 6)],
    });
    render(<StoragePanel />);
    const names = screen.getAllByRole('listitem').map((item) => within(item).getByText(/^Microphone/).textContent);
    expect(names).toEqual(['Microphone 1', 'Microphone 2', 'Microphone 3', 'Microphone 4', 'Microphone 5']);
  });

  it('say so when there are none yet', () => {
    useStorageStore.setState({ sessions: [] });
    render(<StoragePanel />);
    expect(screen.getByText('No sessions recorded yet.')).toBeInTheDocument();
    expect(screen.queryByRole('list')).not.toBeInTheDocument();
  });
});
