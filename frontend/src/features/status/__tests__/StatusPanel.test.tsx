// @vitest-environment jsdom

/**
 * The recorder panel, which answers "is this thing on?": the state badge and how long it has been
 * recording by the server's clock, the Record and Stop button (admins only), the level meter, the
 * support rows, and the errors. The store's start and stop actions are spies; what they send is the
 * store's business.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { Capture, ListenerView, ServiceStatus } from '@/api/types';
import { useAuthStore } from '@/store/useAuthStore';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useListenersStore } from '@/store/useListenersStore';
import { useStatusStore } from '@/store/useStatusStore';

import { StatusPanel } from '../StatusPanel';

const NOW = new Date(2026, 8, 16, 15, 0, 0).getTime();

const RECORDING: Capture = {
  state: 'recording',
  sessionId: 42,
  deviceId: 'usb-mic',
  deviceName: 'USB microphone',
  sampleRate: 48_000,
  channels: 1,
  frameMs: 100,
  startedAtMs: NOW - (3_600_000 + 2 * 60_000 + 5_000),
  droppedFrames: 0,
  error: null,
};

const IDLE: Capture = {
  ...RECORDING,
  state: 'idle',
  sessionId: null,
  sampleRate: 0,
  channels: 0,
  startedAtMs: null,
};

const status = (capture: Capture, overrides: Partial<ServiceStatus> = {}): ServiceStatus => ({
  capture,
  levels: { rms: 0, peak: 0 },
  listeners: 2,
  serverTimeMs: NOW,
  liveEdgeMs: NOW,
  ...overrides,
});

const pristine = {
  auth: useAuthStore.getState(),
  connection: useConnectionStore.getState(),
  listeners: useListenersStore.getState(),
  status: useStatusStore.getState(),
};

let startCapture: Mock<() => Promise<void>>;
let stopCapture: Mock<() => Promise<void>>;

/** The value next to a label in the details list. */
const detail = (label: string) => screen.getByText(label).nextElementSibling;
/** The filled part of the level meter. */
const meterFill = (container: HTMLElement) => container.querySelector('.bg-muted > div') as HTMLElement;

beforeEach(() => {
  useAuthStore.setState(pristine.auth, true);
  useConnectionStore.setState(pristine.connection, true);
  useListenersStore.setState(pristine.listeners, true);
  useStatusStore.setState(pristine.status, true);

  startCapture = vi.fn(async () => undefined);
  stopCapture = vi.fn(async () => undefined);
  useAuthStore.setState({ mode: 'open', user: null });
  useConnectionStore.setState({ connected: true, levels: { rms: 0.1, peak: 0.3 } });
  useStatusStore.setState({
    status: status(RECORDING),
    reachable: true,
    busy: false,
    error: null,
    startCapture,
    stopCapture,
  });
});

describe('the state badge', () => {
  it('says Recording, with how long by the server clock', () => {
    render(<StatusPanel />);
    expect(screen.getByText('Recording')).toBeInTheDocument();
    expect(screen.getByText('01:02:05')).toBeInTheDocument();
  });

  it('never shows a negative time when the recorder clock is behind the start', () => {
    useStatusStore.setState({ status: status({ ...RECORDING, startedAtMs: NOW + 60_000 }) });
    render(<StatusPanel />);
    expect(screen.getByText('00:00:00')).toBeInTheDocument();
  });

  it('says Starting while capture opens the microphone, with no timer', () => {
    useStatusStore.setState({ status: status({ ...RECORDING, state: 'starting' }) });
    render(<StatusPanel />);
    expect(screen.getByText('Starting')).toBeInTheDocument();
    expect(screen.queryByText('01:02:05')).not.toBeInTheDocument();
  });

  it('says Error when capture has failed, with the reason underneath', () => {
    useStatusStore.setState({
      status: status({ ...IDLE, state: 'error', error: 'the microphone was unplugged' }),
    });
    render(<StatusPanel />);
    expect(screen.getByText('Error')).toBeInTheDocument();
    expect(screen.getByText('the microphone was unplugged')).toBeInTheDocument();
  });

  it('says Idle while stopped', () => {
    useStatusStore.setState({ status: status(IDLE) });
    render(<StatusPanel />);
    expect(screen.getByText('Idle')).toBeInTheDocument();
  });

  it('says Idle before the first status arrives', () => {
    useStatusStore.setState({ status: null, reachable: false });
    render(<StatusPanel />);
    expect(screen.getByText('Idle')).toBeInTheDocument();
  });
});

describe('the record and stop button', () => {
  it('stops a running recording', async () => {
    const user = userEvent.setup();
    render(<StatusPanel />);
    await user.click(screen.getByRole('button', { name: 'Stop' }));
    expect(stopCapture).toHaveBeenCalledTimes(1);
    expect(startCapture).not.toHaveBeenCalled();
  });

  it('starts a stopped recorder', async () => {
    const user = userEvent.setup();
    useStatusStore.setState({ status: status(IDLE) });
    render(<StatusPanel />);
    await user.click(screen.getByRole('button', { name: 'Record' }));
    expect(startCapture).toHaveBeenCalledTimes(1);
  });

  it('offers Record again after a capture error', () => {
    useStatusStore.setState({ status: status({ ...IDLE, state: 'error', error: 'gone' }) });
    render(<StatusPanel />);
    expect(screen.getByRole('button', { name: 'Record' })).toBeEnabled();
  });

  it('is disabled while a start or stop is in flight', () => {
    useStatusStore.setState({ busy: true });
    render(<StatusPanel />);
    expect(screen.getByRole('button', { name: 'Stop' })).toBeDisabled();
  });

  it('is disabled while the service cannot be reached', () => {
    useStatusStore.setState({ reachable: false });
    render(<StatusPanel />);
    expect(screen.getByRole('button', { name: 'Stop' })).toBeDisabled();
  });

  it('is not there for a listener, who still sees the state', () => {
    useAuthStore.setState({
      mode: 'accounts',
      user: { id: 2, email: 'kitchen@example.com', role: 'listener', createdAtMs: NOW, twoFactorEnabled: false },
    });
    render(<StatusPanel />);
    expect(screen.queryByRole('button', { name: 'Stop' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Record' })).not.toBeInTheDocument();
    expect(screen.getByText('Recording')).toBeInTheDocument();
  });

  it('is there for an admin account', () => {
    useAuthStore.setState({
      mode: 'accounts',
      user: { id: 1, email: 'owner@example.com', role: 'admin', createdAtMs: NOW, twoFactorEnabled: false },
    });
    render(<StatusPanel />);
    expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument();
  });
});

describe('the level meter', () => {
  it('moves while recording with the broadcast link up', () => {
    const { container } = render(<StatusPanel />);
    expect(meterFill(container).style.width).not.toBe('0%');
  });

  it('rests at zero while stopped', () => {
    useStatusStore.setState({ status: status(IDLE) });
    const { container } = render(<StatusPanel />);
    expect(meterFill(container).style.width).toBe('0%');
  });

  it('rests at zero while the broadcast link is down', () => {
    useConnectionStore.setState({ connected: false });
    const { container } = render(<StatusPanel />);
    expect(meterFill(container).style.width).toBe('0%');
  });
});

describe('the details', () => {
  it('give the format as sample rate and channels', () => {
    render(<StatusPanel />);
    expect(detail('Format')).toHaveTextContent('48.0 kHz, mono');
  });

  it('count channels beyond mono', () => {
    useStatusStore.setState({ status: status({ ...RECORDING, sampleRate: 44_100, channels: 2 }) });
    render(<StatusPanel />);
    expect(detail('Format')).toHaveTextContent('44.1 kHz, 2 ch');
  });

  it('say not started before capture has a format', () => {
    useStatusStore.setState({ status: status(IDLE) });
    render(<StatusPanel />);
    expect(detail('Format')).toHaveTextContent('not started');
  });

  it('count listeners from the polled status', () => {
    render(<StatusPanel />);
    expect(detail('Listeners')).toHaveTextContent('2');
  });

  it('count listeners from the pushed list when there is one', () => {
    const tab: ListenerView = {
      id: 1,
      email: null,
      role: null,
      address: '10.0.0.5',
      userAgent: null,
      connectedAtMs: NOW,
      activity: 'live',
      fromMs: null,
      player: 'playing',
    };
    useListenersStore.setState({ listeners: [tab, { ...tab, id: 2 }, { ...tab, id: 3 }] });
    render(<StatusPanel />);
    expect(detail('Listeners')).toHaveTextContent('3');
  });

  it('name the session, or none', () => {
    const { unmount } = render(<StatusPanel />);
    expect(detail('Session')).toHaveTextContent('42');
    unmount();

    useStatusStore.setState({ status: status(IDLE) });
    render(<StatusPanel />);
    expect(detail('Session')).toHaveTextContent('none');
  });

  it('count dropped frames', () => {
    useStatusStore.setState({ status: status({ ...RECORDING, droppedFrames: 17 }) });
    render(<StatusPanel />);
    expect(detail('Dropped frames')).toHaveTextContent('17');
  });

  it('say whether the broadcast link is connected or reconnecting', () => {
    render(<StatusPanel />);
    expect(detail('Broadcast link')).toHaveTextContent('connected');

    act(() => {
      useConnectionStore.setState({ connected: false });
    });
    expect(detail('Broadcast link')).toHaveTextContent('reconnecting');
  });
});

describe('when the service cannot be reached', () => {
  it('says why', () => {
    useStatusStore.setState({ reachable: false, error: 'the service is unreachable' });
    render(<StatusPanel />);
    expect(screen.getByText('the service is unreachable')).toBeInTheDocument();
  });

  // A start that fails on the host comes back as the capture error on the next poll, shown above.
  it('keeps the outage message for when the service cannot be reached', () => {
    useStatusStore.setState({ reachable: true, error: 'could not start capture' });
    render(<StatusPanel />);
    expect(screen.queryByText('could not start capture')).not.toBeInTheDocument();
  });
});
