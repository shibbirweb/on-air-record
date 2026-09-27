// @vitest-environment jsdom

/**
 * The microphone chooser: it lists the system default and every device, marks the default and the
 * unplugged ones, sends the choice (or no choice, for the system default), rescans on request, warns when
 * the configured device is missing, names the microphone actually being captured, and is read only for a
 * listener. The store's actions are spies; the requests are the store's.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { Capture, InputDevice } from '@/api/types';
import { useAuthStore } from '@/store/useAuthStore';
import { useDeviceStore } from '@/store/useDeviceStore';
import { useStatusStore } from '@/store/useStatusStore';

import { DeviceSelector } from '../DeviceSelector';

const device = (overrides: Partial<InputDevice> & Pick<InputDevice, 'id' | 'name'>): InputDevice => ({
  isDefault: false,
  isSelected: false,
  available: true,
  channels: 1,
  sampleRate: 48_000,
  ...overrides,
});

const BUILT_IN = device({ id: 'built-in', name: 'MacBook Pro Microphone', isDefault: true });
const USB = device({ id: 'usb', name: 'USB Audio Device' });
const OLD_HEADSET = device({ id: 'headset', name: 'Old headset', available: false });

const CAPTURE: Capture = {
  state: 'recording',
  sessionId: 3,
  deviceId: 'built-in',
  deviceName: 'MacBook Pro Microphone',
  sampleRate: 48_000,
  deviceSampleRate: 48_000,
  channels: 1,
  frameMs: 100,
  startedAtMs: 0,
  droppedFrames: 0,
  error: null,
};

const pristine = {
  auth: useAuthStore.getState(),
  devices: useDeviceStore.getState(),
  status: useStatusStore.getState(),
};

let refresh: Mock<() => Promise<void>>;
let select: Mock<(deviceId: string | null) => Promise<void>>;

const chooser = () => screen.getByRole('combobox', { name: 'Input source' });

function setStatus(capture: Capture | null) {
  useStatusStore.setState({
    status:
      capture === null
        ? null
        : { capture, levels: { rms: 0, peak: 0 }, listeners: 0, serverTimeMs: 0, liveEdgeMs: null },
  });
}

beforeEach(() => {
  useAuthStore.setState(pristine.auth, true);
  useDeviceStore.setState(pristine.devices, true);
  useStatusStore.setState(pristine.status, true);

  refresh = vi.fn(async () => undefined);
  select = vi.fn(async () => undefined);
  useAuthStore.setState({ mode: 'open', user: null });
  useDeviceStore.setState({
    devices: [BUILT_IN, USB, OLD_HEADSET],
    loading: false,
    selecting: false,
    error: null,
    refresh,
    select,
  });
  setStatus(CAPTURE);
});

describe('the device list', () => {
  it('is fetched when the panel appears', () => {
    render(<DeviceSelector />);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('shows System default when no device has been chosen', () => {
    render(<DeviceSelector />);
    expect(chooser()).toHaveTextContent('System default');
  });

  it('shows the chosen device', () => {
    useDeviceStore.setState({ devices: [BUILT_IN, { ...USB, isSelected: true }, OLD_HEADSET] });
    render(<DeviceSelector />);
    expect(chooser()).toHaveTextContent('USB Audio Device');
  });

  it('offers the system default first, then each device, marking the default and the unplugged', async () => {
    const user = userEvent.setup();
    render(<DeviceSelector />);
    await user.click(chooser());

    const options = screen.getAllByRole('option');
    expect(options.map((option) => option.textContent)).toEqual([
      'System default',
      'MacBook Pro Microphone (default)',
      'USB Audio Device',
      'Old headset (unplugged)',
    ]);
    expect(screen.getByRole('option', { name: 'Old headset (unplugged)' })).toHaveAttribute('aria-disabled', 'true');
  });

  it('switches to a chosen device', async () => {
    const user = userEvent.setup();
    render(<DeviceSelector />);
    await user.click(chooser());
    await user.click(screen.getByRole('option', { name: 'USB Audio Device' }));
    expect(select).toHaveBeenCalledWith('usb');
  });

  it('goes back to the system default by choosing no device', async () => {
    const user = userEvent.setup();
    useDeviceStore.setState({ devices: [BUILT_IN, { ...USB, isSelected: true }] });
    render(<DeviceSelector />);
    await user.click(chooser());
    await user.click(screen.getByRole('option', { name: 'System default' }));
    expect(select).toHaveBeenCalledWith(null);
  });

  it('cannot be changed while a switch is in progress', () => {
    useDeviceStore.setState({ selecting: true });
    render(<DeviceSelector />);
    expect(chooser()).toBeDisabled();
  });
});

describe('the rescan button', () => {
  it('looks for devices again', async () => {
    const user = userEvent.setup();
    render(<DeviceSelector />);
    await user.click(screen.getByRole('button', { name: 'Rescan devices' }));
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it('is disabled and spinning while a scan runs', () => {
    useDeviceStore.setState({ loading: true });
    render(<DeviceSelector />);
    const button = screen.getByRole('button', { name: 'Rescan devices' });
    expect(button).toBeDisabled();
    expect(button.querySelector('svg')).toHaveClass('animate-spin');
  });
});

describe('the warnings', () => {
  it('say when the configured device is not connected', () => {
    useDeviceStore.setState({ devices: [BUILT_IN, { ...OLD_HEADSET, isSelected: true }] });
    render(<DeviceSelector />);
    expect(screen.getByText(/The configured device is not connected/)).toBeInTheDocument();
  });

  it('stay away while the configured device is plugged in', () => {
    useDeviceStore.setState({ devices: [BUILT_IN, { ...USB, isSelected: true }] });
    render(<DeviceSelector />);
    expect(screen.queryByText(/The configured device is not connected/)).not.toBeInTheDocument();
  });

  it('show an error from the last scan or switch', () => {
    useDeviceStore.setState({ error: 'could not select that device' });
    render(<DeviceSelector />);
    expect(screen.getByText('could not select that device')).toBeInTheDocument();
  });
});

describe('the microphone being captured', () => {
  it('is named, highlighted as live while recording', () => {
    render(<DeviceSelector />);
    expect(screen.getByText('Capturing from')).toBeInTheDocument();
    const badge = screen.getByText('MacBook Pro Microphone', { selector: '[data-slot="badge"]' });
    expect(badge).toHaveClass('bg-[var(--live)]');
  });

  it('is named without the live colour while capture is not recording', () => {
    setStatus({ ...CAPTURE, state: 'starting' });
    render(<DeviceSelector />);
    const badge = screen.getByText('MacBook Pro Microphone', { selector: '[data-slot="badge"]' });
    expect(badge).toHaveClass('bg-secondary');
    expect(badge).not.toHaveClass('bg-[var(--live)]');
  });

  it('is not mentioned before capture has named a device', () => {
    setStatus({ ...CAPTURE, state: 'idle', deviceName: null });
    render(<DeviceSelector />);
    expect(screen.queryByText('Capturing from')).not.toBeInTheDocument();
  });
});

describe('for a listener', () => {
  beforeEach(() => {
    useAuthStore.setState({
      mode: 'accounts',
      user: { id: 2, email: 'kitchen@example.com', role: 'listener', createdAtMs: 0, twoFactorEnabled: false },
    });
  });

  it('shows the microphone in use but cannot change it', () => {
    render(<DeviceSelector />);
    expect(chooser()).toBeDisabled();
    expect(chooser()).toHaveTextContent('System default');
    expect(screen.getByText('Capturing from')).toBeInTheDocument();
  });

  it('can still rescan, which changes nothing on the host', () => {
    render(<DeviceSelector />);
    expect(screen.getByRole('button', { name: 'Rescan devices' })).toBeEnabled();
  });
});
