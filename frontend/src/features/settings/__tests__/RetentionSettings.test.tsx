// @vitest-environment jsdom

/**
 * How long recordings are kept, and the storage projection beside it. The window can be typed in hours or
 * days, picked from a preset, or switched to forever; every route has to land in the settings draft as a
 * number of hours (or null for forever) and wait for Save. The projection is the reason the card exists,
 * so its figures are checked against the exact PCM arithmetic: at 48 kHz an hour is 345.6 million bytes,
 * shown as 329.6 MB, and a day is 7.7 GB.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';

import { useSettingsStore } from '@/store/useSettingsStore';
import { useStatusStore } from '@/store/useStatusStore';
import { useStorageStore } from '@/store/useStorageStore';

import { RetentionSettings } from '../RetentionSettings';
import { serviceStatus, storageUsage, STORED } from './fixtures';

const FIVE_GB = 5 * 1024 ** 3;
const BYTES_PER_HOUR_48K = 48_000 * 2 * 3600;

const amount = () => screen.getByRole('spinbutton', { name: 'Retention amount' });
const limitedRadio = () => screen.getByRole('radio', { name: 'Delete recordings older than' });
const foreverRadio = () => screen.getByRole('radio', { name: 'Keep everything forever' });
const draftHours = () => useSettingsStore.getState().draft.retentionHours;

/** The figure beside a term in the storage list. */
function figure(term: string): HTMLElement {
  const value = screen.getByText(term).nextElementSibling;
  if (!(value instanceof HTMLElement)) {
    throw new Error(`no figure for ${term}`);
  }
  return value;
}

function renderCard() {
  const user = userEvent.setup();
  render(<RetentionSettings />);
  return user;
}

describe('the retention settings card', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, defaults: STORED, draft: {}, error: null });
    useStatusStore.setState({ status: serviceStatus(48_000) });
    useStorageStore.setState({ storage: storageUsage(FIVE_GB, BYTES_PER_HOUR_48K) });
  });

  it('says it is loading until the settings arrive', () => {
    useSettingsStore.setState({ settings: null });
    render(<RetentionSettings />);
    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  describe('showing the window', () => {
    it('shows a stored window of 24 hours in hours', () => {
      render(<RetentionSettings />);
      expect(limitedRadio()).toBeChecked();
      expect(foreverRadio()).not.toBeChecked();
      expect(amount()).toHaveValue(24);
    });

    it('shows a whole number of days past one day in days', () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 720 } });
      render(<RetentionSettings />);
      expect(amount()).toHaveValue(30);
    });

    it('keeps a window that is not whole days in hours', () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 36 } });
      render(<RetentionSettings />);
      expect(amount()).toHaveValue(36);
    });

    it('shows a pending window over the stored one', () => {
      useSettingsStore.setState({ draft: { retentionHours: 168 } });
      render(<RetentionSettings />);
      expect(amount()).toHaveValue(7);
    });

    it('shows keeping forever as the chosen option', () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: null } });
      render(<RetentionSettings />);
      expect(foreverRadio()).toBeChecked();
      expect(limitedRadio()).not.toBeChecked();
    });
  });

  describe('changing the window', () => {
    it('stages a preset as a number of hours', async () => {
      const user = renderCard();
      await user.click(screen.getByRole('button', { name: '7 days' }));
      expect(draftHours()).toBe(168);
      expect(amount()).toHaveValue(7);
    });

    it('offers presets from six hours to a year', () => {
      render(<RetentionSettings />);
      for (const label of ['6 hours', '24 hours', '3 days', '7 days', '30 days', '90 days', '1 year']) {
        expect(screen.getByRole('button', { name: label })).toBeInTheDocument();
      }
    });

    it('stages a year as 8760 hours', async () => {
      const user = renderCard();
      await user.click(screen.getByRole('button', { name: '1 year' }));
      expect(draftHours()).toBe(8760);
    });

    it('stages nothing when the stored preset is picked again', async () => {
      useSettingsStore.setState({ draft: { retentionHours: 720 } });
      const user = renderCard();
      await user.click(screen.getByRole('button', { name: '24 hours' }));
      expect(useSettingsStore.getState().draft).toEqual({});
    });

    it('stages a typed number of hours when the box loses focus', async () => {
      const user = renderCard();
      await user.clear(amount());
      await user.type(amount(), '36');
      expect(useSettingsStore.getState().draft).toEqual({});

      await user.tab();
      expect(draftHours()).toBe(36);
      expect(amount()).toHaveValue(36);
    });

    it('stages a typed number when Enter is pressed', async () => {
      const user = renderCard();
      await user.clear(amount());
      await user.type(amount(), '12{Enter}');
      expect(draftHours()).toBe(12);
    });

    it('reads a typed number as days while days are shown', async () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 72 } });
      const user = renderCard();
      await user.clear(amount());
      await user.type(amount(), '5{Enter}');
      expect(draftHours()).toBe(120);
      expect(amount()).toHaveValue(5);
    });

    it('shows typed hours that make whole days back in days', async () => {
      const user = renderCard();
      await user.clear(amount());
      await user.type(amount(), '48{Enter}');
      expect(draftHours()).toBe(48);
      expect(amount()).toHaveValue(2);
    });

    it('ignores an empty box and puts the stored window back', async () => {
      const user = renderCard();
      await user.clear(amount());
      await user.tab();
      expect(useSettingsStore.getState().draft).toEqual({});
      expect(amount()).toHaveValue(24);
    });

    it('ignores zero and puts the stored window back', async () => {
      const user = renderCard();
      await user.clear(amount());
      await user.type(amount(), '0');
      await user.tab();
      expect(useSettingsStore.getState().draft).toEqual({});
      expect(amount()).toHaveValue(24);
    });

    it('converts the window when switching to days rather than reinterpreting the number', async () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 36 } });
      const user = renderCard();
      await user.click(screen.getByRole('button', { name: 'days' }));
      // A day and a half rounds to two days, never to 36 days.
      expect(draftHours()).toBe(48);
      expect(amount()).toHaveValue(2);
    });

    it('converts the window when switching to hours', async () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 720 } });
      const user = renderCard();
      await user.click(screen.getByRole('button', { name: 'hours' }));
      expect(amount()).toHaveValue(720);
      // The same window in another unit is not a change.
      expect(useSettingsStore.getState().draft).toEqual({});
    });

    it('never turns a window into zero days', async () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 6 } });
      const user = renderCard();
      await user.click(screen.getByRole('button', { name: 'days' }));
      expect(draftHours()).toBe(24);
      expect(amount()).toHaveValue(1);
    });

    it('stages keeping forever as an explicit null', async () => {
      const user = renderCard();
      await user.click(foreverRadio());
      const { draft } = useSettingsStore.getState();
      expect('retentionHours' in draft).toBe(true);
      expect(draft.retentionHours).toBeNull();
      expect(foreverRadio()).toBeChecked();
    });

    it('stages a day when leaving forever for a window', async () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: null } });
      const user = renderCard();
      await user.click(limitedRadio());
      expect(draftHours()).toBe(24);
      expect(limitedRadio()).toBeChecked();
    });
  });

  describe('the storage projection', () => {
    it('shows the recording rate, what is used and what a full window needs', () => {
      render(<RetentionSettings />);
      expect(figure('Recording rate')).toHaveTextContent('329.6 MB per hour');
      expect(figure('Used right now')).toHaveTextContent('5.0 GB');
      expect(figure('Needed when full')).toHaveTextContent('7.7 GB');
      expect(screen.getByText(/at 96 kB per second of uncompressed audio/)).toBeInTheDocument();
    });

    it('projects the window being typed before it is committed', async () => {
      const user = renderCard();
      await user.clear(amount());
      await user.type(amount(), '48');
      expect(figure('Needed when full')).toHaveTextContent('15.4 GB');
      expect(useSettingsStore.getState().draft).toEqual({});
    });

    it('projects a pending window', () => {
      useSettingsStore.setState({ draft: { retentionHours: 720 } });
      render(<RetentionSettings />);
      expect(figure('Needed when full')).toHaveTextContent('231.7 GB');
    });

    it('states the chosen rate per second even before storage has loaded', () => {
      useStorageStore.setState({ storage: null });
      useSettingsStore.setState({ draft: { recordingSampleRate: 16_000 } });
      render(<RetentionSettings />);
      expect(screen.getByText(/at 32 kB per second of uncompressed audio/)).toBeInTheDocument();
      expect(screen.queryByText(/96 kB per second/)).not.toBeInTheDocument();
    });

    it('projects at a pending bit rate, so a lower rate and a longer window combine before saving', () => {
      useSettingsStore.setState({ draft: { recordingSampleRate: 16_000, retentionHours: 168 } });
      render(<RetentionSettings />);
      expect(figure('Recording rate')).toHaveTextContent('109.9 MB per hour');
      expect(figure('Needed when full')).toHaveTextContent('18.0 GB');
      expect(screen.getByText(/at 32 kB per second of uncompressed audio/)).toBeInTheDocument();
    });

    it('projects Match the device at the microphone rate while recording at a lower one', () => {
      // Recording at 8 kHz from a 48 kHz microphone, and choosing to match the device again.
      useSettingsStore.setState({
        settings: { ...STORED, recordingSampleRate: 8_000 },
        draft: { recordingSampleRate: null },
      });
      useStatusStore.setState({ status: serviceStatus(8_000, 'recording', 48_000) });
      render(<RetentionSettings />);
      expect(figure('Recording rate')).toHaveTextContent('329.6 MB per hour');
      expect(screen.getByText(/at 96 kB per second of uncompressed audio/)).toBeInTheDocument();
    });

    it('falls back to the rate the server reports while the microphone rate is unknown', () => {
      useStatusStore.setState({ status: null });
      useStorageStore.setState({ storage: storageUsage(0, 24_000 * 2 * 3600) });
      render(<RetentionSettings />);
      expect(figure('Recording rate')).toHaveTextContent('164.8 MB per hour');
      expect(figure('Used right now')).toHaveTextContent('0 B');
    });

    it('predicts no ceiling when keeping forever, and warns how fast the disk fills', () => {
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: null } });
      render(<RetentionSettings />);
      expect(figure('Needed when full')).toHaveTextContent('grows without limit');
      expect(screen.getByText(/With no window there is no ceiling to predict/)).toBeInTheDocument();
      expect(
        screen.getByText(/Keeping forever fills the disk in 7\.7 GB per day of continuous recording/),
      ).toBeInTheDocument();
    });

    it('shows the forever warning as soon as forever is picked, before saving', async () => {
      const user = renderCard();
      expect(screen.queryByText(/Nothing will stop it/)).not.toBeInTheDocument();
      await user.click(foreverRadio());
      expect(screen.getByText(/Nothing will stop it/)).toBeInTheDocument();
    });
  });
});
