// @vitest-environment jsdom

/**
 * The recording bit rate menu: which rates it offers for the microphone in use, what each costs per hour,
 * the projection under it, and that a choice is staged in the settings draft for Save. Recordings are raw
 * 16 bit mono PCM, so every figure here is exact arithmetic (48 kHz is 768 kbps is 345.6 million bytes an
 * hour), which is why the tests can state the numbers rather than compute them.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';

import { useSettingsStore } from '@/store/useSettingsStore';
import { useStatusStore } from '@/store/useStatusStore';

import { RecordingQuality } from '../RecordingQuality';
import { serviceStatus, STORED } from './fixtures';

const trigger = () => screen.getByRole('combobox', { name: 'Recording bit rate' });

async function openMenu() {
  const user = userEvent.setup();
  render(<RecordingQuality />);
  await user.click(trigger());
  return user;
}

describe('the recording bit rate menu', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, defaults: STORED, draft: {}, error: null });
    useStatusStore.setState({ status: serviceStatus(48_000) });
  });

  it('says it is loading until the settings arrive', () => {
    useSettingsStore.setState({ settings: null });
    render(<RecordingQuality />);
    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('shows Match the device when no rate is stored, with what the device gives', () => {
    render(<RecordingQuality />);
    expect(trigger()).toHaveTextContent('Match the device');
    expect(screen.getByText(/768 kbps is 329\.6 MB per hour/)).toBeInTheDocument();
  });

  it('shows a stored rate and projects the disk it uses', () => {
    useSettingsStore.setState({ settings: { ...STORED, recordingSampleRate: 16_000 } });
    render(<RecordingQuality />);
    expect(trigger()).toHaveTextContent('Voice, 256 kbps');
    expect(screen.getByText(/256 kbps is 109\.9 MB per hour/)).toBeInTheDocument();
  });

  it('shows a pending rate over the stored one', () => {
    useSettingsStore.setState({ draft: { recordingSampleRate: 8_000 } });
    render(<RecordingQuality />);
    expect(trigger()).toHaveTextContent('Telephone, 128 kbps');
    expect(screen.getByText(/128 kbps is 54\.9 MB per hour/)).toBeInTheDocument();
  });

  it('projects no more than the device produces when the stored rate is above it', () => {
    useSettingsStore.setState({ settings: { ...STORED, recordingSampleRate: 48_000 } });
    useStatusStore.setState({ status: serviceStatus(16_000) });
    render(<RecordingQuality />);
    expect(screen.getByText(/256 kbps is 109\.9 MB per hour/)).toBeInTheDocument();
  });

  it('waits for capture to start before giving a figure for matching the device', () => {
    useStatusStore.setState({ status: null });
    render(<RecordingQuality />);
    expect(screen.getByText(/the figure appears once capture has started/)).toBeInTheDocument();
  });

  it('gives the figure for a fixed rate even before capture has started', () => {
    useStatusStore.setState({ status: null });
    useSettingsStore.setState({ settings: { ...STORED, recordingSampleRate: 24_000 } });
    render(<RecordingQuality />);
    expect(screen.getByText(/384 kbps is 164\.8 MB per hour/)).toBeInTheDocument();
  });

  it('offers every rate from full quality to telephone for a 48 kHz microphone', async () => {
    await openMenu();
    const options = screen.getAllByRole('option');
    expect(options.map((option) => option.textContent)).toEqual([
      expect.stringContaining('Match the device'),
      expect.stringContaining('Full quality, 768 kbps'),
      expect.stringContaining('High, 512 kbps'),
      expect.stringContaining('Good, 384 kbps'),
      expect.stringContaining('Voice, 256 kbps'),
      expect.stringContaining('Telephone, 128 kbps'),
    ]);
  });

  it('offers nothing above what the microphone produces', async () => {
    useStatusStore.setState({ status: serviceStatus(24_000) });
    await openMenu();
    expect(screen.queryByRole('option', { name: /Full quality/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('option', { name: /^High/ })).not.toBeInTheDocument();
    expect(screen.getByRole('option', { name: /^Good, 384 kbps/ })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: /^Telephone/ })).toBeInTheDocument();
  });

  it('keeps offering every rate the microphone can give while recording at a lower one', async () => {
    // Recording at Telephone quality: the recorder reports 8 kHz, the microphone still runs at 48 kHz.
    // Offering only what lies under the recording rate trapped the choice at Telephone.
    useSettingsStore.setState({ settings: { ...STORED, recordingSampleRate: 8_000 } });
    useStatusStore.setState({ status: serviceStatus(8_000, 'recording', 48_000) });
    await openMenu();
    expect(screen.getAllByRole('option')).toHaveLength(6);
    expect(screen.getByRole('option', { name: /^Full quality, 768 kbps/ })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: /^Match the device/ })).toHaveTextContent('768 kbps');
  });

  it('offers every rate while the microphone rate is unknown', async () => {
    useStatusStore.setState({ status: null });
    await openMenu();
    expect(screen.getAllByRole('option')).toHaveLength(6);
    expect(screen.getByRole('option', { name: /^Match the device/ })).toHaveTextContent(
      'whatever the microphone offers',
    );
  });

  it('tells each option its cost per hour and what it suits', async () => {
    await openMenu();
    expect(screen.getByRole('option', { name: /^Match the device/ })).toHaveTextContent(
      '768 kbps at 48.0 kHz',
    );
    expect(screen.getByRole('option', { name: /^Voice/ })).toHaveTextContent(
      /16 kHz . 109\.9 MB per hour . speech, a third of the disk/,
    );
    expect(screen.getByRole('option', { name: /^Full quality/ })).toHaveTextContent(
      /48 kHz . 329\.6 MB per hour . music and detail/,
    );
  });

  it('stages a lower rate in the draft, to wait for Save', async () => {
    const user = await openMenu();
    await user.click(screen.getByRole('option', { name: /^Voice/ }));

    expect(useSettingsStore.getState().draft).toEqual({ recordingSampleRate: 16_000 });
    expect(trigger()).toHaveTextContent('Voice, 256 kbps');
    expect(screen.getByText(/256 kbps is 109\.9 MB per hour/)).toBeInTheDocument();
  });

  it('stages going back to the device rate as an explicit null', async () => {
    useSettingsStore.setState({ settings: { ...STORED, recordingSampleRate: 16_000 } });
    const user = await openMenu();
    await user.click(screen.getByRole('option', { name: /^Match the device/ }));

    const { draft } = useSettingsStore.getState();
    expect('recordingSampleRate' in draft).toBe(true);
    expect(draft.recordingSampleRate).toBeNull();
  });

  it('stages nothing when the stored rate is chosen again', async () => {
    useSettingsStore.setState({ draft: { recordingSampleRate: 8_000 } });
    const user = await openMenu();
    await user.click(screen.getByRole('option', { name: /^Match the device/ }));
    expect(useSettingsStore.getState().draft).toEqual({});
  });

  it('says a change applies to the next session and leaves existing recordings alone', () => {
    render(<RecordingQuality />);
    expect(
      screen.getByText(/Applies to the next recording session, and existing recordings keep their own rate/),
    ).toBeInTheDocument();
  });
});
