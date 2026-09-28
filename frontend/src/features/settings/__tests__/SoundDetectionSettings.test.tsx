// @vitest-environment jsdom

/**
 * The sound detection card: it shows what is stored or pending, and a choice goes into the settings draft
 * to wait for Save, like every other setting. The real settings store is used, so the draft rules (only
 * what differs from the stored value is staged) are part of what is tested.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';

import type { Settings } from '@/api/types';
import { useSettingsStore } from '@/store/useSettingsStore';

import { SoundDetectionSettings } from '../SoundDetectionSettings';

const STORED: Settings = {
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

const trigger = () => screen.getByRole('combobox', { name: 'Sound detection' });

describe('the sound detection card', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, defaults: STORED, draft: {}, error: null });
  });

  it('shows the stored level', () => {
    useSettingsStore.setState({ settings: { ...STORED, soundSensitivity: 'high' } });
    render(<SoundDetectionSettings />);
    expect(trigger()).toHaveTextContent('High');
  });

  it('shows a pending choice over the stored one', () => {
    useSettingsStore.setState({ draft: { soundSensitivity: 'low' } });
    render(<SoundDetectionSettings />);
    expect(trigger()).toHaveTextContent('Low');
  });

  it('falls back to Medium before the settings have loaded', () => {
    useSettingsStore.setState({ settings: null });
    render(<SoundDetectionSettings />);
    expect(trigger()).toHaveTextContent('Medium');
  });

  it('offers the three levels, each explained', async () => {
    const user = userEvent.setup();
    render(<SoundDetectionSettings />);
    await user.click(trigger());

    const options = screen.getAllByRole('option');
    expect(options.map((option) => option.textContent)).toEqual([
      expect.stringContaining('Low'),
      expect.stringContaining('Medium'),
      expect.stringContaining('High'),
    ]);
    expect(options[0]).toHaveTextContent('a door or a raised voice');
    expect(options[2]).toHaveTextContent('quiet sounds too');
  });

  it('stages a new choice in the draft, to wait for Save', async () => {
    const user = userEvent.setup();
    render(<SoundDetectionSettings />);
    await user.click(trigger());
    await user.click(screen.getByRole('option', { name: /^High/ }));

    expect(useSettingsStore.getState().draft).toEqual({ soundSensitivity: 'high' });
    expect(trigger()).toHaveTextContent('High');
  });

  it('stages nothing when the stored level is chosen again', async () => {
    useSettingsStore.setState({ draft: { soundSensitivity: 'low' } });
    const user = userEvent.setup();
    render(<SoundDetectionSettings />);
    await user.click(trigger());
    await user.click(screen.getByRole('option', { name: /^Medium/ }));

    expect(useSettingsStore.getState().draft).toEqual({});
  });

  it('points people who miss quiet sounds at High and the gain', () => {
    render(<SoundDetectionSettings />);
    expect(screen.getByText(/choose High, or raise the input gain/)).toBeInTheDocument();
  });
});
