// @vitest-environment jsdom

/**
 * The Save bar at the foot of the settings page: what it says about pending changes, which buttons it
 * allows when, what Save sends, the second question before a save that would delete audio, and how it
 * reports a refusal. The real settings and accounts stores are used, with the server calls replaced by
 * spies, because what the bar does is decided by the drafts those stores hold.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { api, ApiError } from '@/api/client';
import type { Settings, SettingsPatch, User } from '@/api/types';
import { useAccountsStore } from '@/store/useAccountsStore';
import { useSettingsStore } from '@/store/useSettingsStore';

import { SettingsActionBar } from '../SettingsActionBar';
import { account, STORED } from './fixtures';

const USERS: User[] = [account(1, 'admin'), account(2, 'listener')];

const saveButton = () => screen.getByRole('button', { name: /Save changes|Saving/ });
const discardButton = () => screen.getByRole('button', { name: 'Discard' });
const restoreButton = () => screen.getByRole('button', { name: 'Restore defaults' });

/** The server's answer to a settings save: what was stored with the patch applied. */
function acceptSettings() {
  return vi
    .spyOn(api, 'updateSettings')
    .mockImplementation(async (patch: SettingsPatch): Promise<Settings> => ({
      ...(useSettingsStore.getState().settings ?? STORED),
      ...patch,
    }));
}

function renderBar() {
  const user = userEvent.setup();
  render(<SettingsActionBar />);
  return user;
}

describe('the settings save bar', () => {
  beforeEach(() => {
    useSettingsStore.setState({
      settings: STORED,
      defaults: STORED,
      draft: {},
      saving: false,
      error: null,
    });
    useAccountsStore.setState({
      users: USERS,
      roleDraft: {},
      savingRoles: false,
      roleError: null,
      error: null,
    });
    vi.spyOn(api, 'users').mockImplementation(async () => USERS.map((user) => ({ ...user })));
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('with nothing pending', () => {
    it('says everything is at its default and allows nothing', () => {
      render(<SettingsActionBar />);
      expect(screen.getByText('Everything is at its default.')).toBeInTheDocument();
      expect(saveButton()).toBeDisabled();
      expect(discardButton()).toBeDisabled();
      expect(restoreButton()).toBeDisabled();
    });

    it('says all changes are saved once something differs from the defaults', () => {
      useSettingsStore.setState({ settings: { ...STORED, gain: 2 } });
      render(<SettingsActionBar />);
      expect(screen.getByText('All changes saved.')).toBeInTheDocument();
      expect(restoreButton()).toBeEnabled();
      expect(saveButton()).toBeDisabled();
    });

    it('counts a stored sound detection level other than the default as away from the defaults', () => {
      useSettingsStore.setState({ settings: { ...STORED, soundSensitivity: 'high' } });
      render(<SettingsActionBar />);
      expect(screen.getByText('All changes saved.')).toBeInTheDocument();
    });

    it('does not claim to be at the defaults before the defaults have loaded', () => {
      useSettingsStore.setState({ defaults: null });
      render(<SettingsActionBar />);
      expect(screen.getByText('All changes saved.')).toBeInTheDocument();
    });
  });

  describe('counting pending changes', () => {
    it('counts one change in the singular', () => {
      useSettingsStore.setState({ draft: { gain: 2 } });
      render(<SettingsActionBar />);
      expect(screen.getByText('1 unsaved change')).toBeInTheDocument();
      expect(saveButton()).toBeEnabled();
      expect(discardButton()).toBeEnabled();
    });

    it('counts several changes', () => {
      useSettingsStore.setState({ draft: { gain: 2, autoStart: false } });
      render(<SettingsActionBar />);
      expect(screen.getByText('2 unsaved changes')).toBeInTheDocument();
    });

    it('counts staged account roles together with the settings', () => {
      useSettingsStore.setState({ draft: { gain: 2 } });
      useAccountsStore.setState({ roleDraft: { 2: 'admin' } });
      render(<SettingsActionBar />);
      expect(screen.getByText('2 unsaved changes')).toBeInTheDocument();
    });

    it('counts a staged role alone as something to save', () => {
      useAccountsStore.setState({ roleDraft: { 2: 'admin' } });
      render(<SettingsActionBar />);
      expect(screen.getByText('1 unsaved change')).toBeInTheDocument();
      expect(saveButton()).toBeEnabled();
    });
  });

  describe('Restore defaults', () => {
    it('stages the defaults for review instead of saving them', async () => {
      const save = acceptSettings();
      useSettingsStore.setState({ settings: { ...STORED, gain: 3, retentionHours: 168 } });
      const user = renderBar();
      await user.click(restoreButton());

      expect(useSettingsStore.getState().draft).toEqual({ gain: 1, retentionHours: 24 });
      expect(screen.getByText('2 unsaved changes')).toBeInTheDocument();
      expect(save).not.toHaveBeenCalled();
    });

    it('is unavailable once the pending values are the defaults', async () => {
      useSettingsStore.setState({ settings: { ...STORED, gain: 3 } });
      const user = renderBar();
      await user.click(restoreButton());
      expect(restoreButton()).toBeDisabled();
    });

    it('stays available until the recording quality and the sensitivity are back too, then resets them', async () => {
      useSettingsStore.setState({
        settings: { ...STORED, recordingSampleRate: 16_000, soundSensitivity: 'high' },
      });
      const user = renderBar();
      expect(restoreButton()).toBeEnabled();
      await user.click(restoreButton());

      expect(useSettingsStore.getState().draft).toEqual({
        recordingSampleRate: STORED.recordingSampleRate,
        soundSensitivity: STORED.soundSensitivity,
      });
      expect(restoreButton()).toBeDisabled();
    });
  });

  describe('Discard', () => {
    it('drops pending settings and staged roles alike', async () => {
      useSettingsStore.setState({ draft: { gain: 2 }, error: 'the directory is not writable' });
      useAccountsStore.setState({ roleDraft: { 2: 'admin' }, roleError: 'refused' });
      const user = renderBar();
      await user.click(discardButton());

      expect(useSettingsStore.getState().draft).toEqual({});
      expect(useAccountsStore.getState().roleDraft).toEqual({});
      expect(screen.getByText('Everything is at its default.')).toBeInTheDocument();
      expect(screen.queryByText('the directory is not writable')).not.toBeInTheDocument();
    });
  });

  describe('Save', () => {
    it('sends the whole settings draft in one request and then reads as saved', async () => {
      const save = acceptSettings();
      const roles = vi.spyOn(api, 'updateUserRole');
      useSettingsStore.setState({ draft: { gain: 2, autoStart: false } });
      const user = renderBar();
      await user.click(saveButton());

      expect(save).toHaveBeenCalledTimes(1);
      expect(save).toHaveBeenCalledWith({ gain: 2, autoStart: false });
      expect(roles).not.toHaveBeenCalled();
      expect(useSettingsStore.getState().settings?.gain).toBe(2);
      expect(screen.getByText('All changes saved.')).toBeInTheDocument();
    });

    it('applies staged roles without a settings request when only roles changed', async () => {
      const save = acceptSettings();
      const roles = vi.spyOn(api, 'updateUserRole').mockResolvedValue(account(2, 'admin'));
      useAccountsStore.setState({ roleDraft: { 2: 'admin' } });
      const user = renderBar();
      await user.click(saveButton());

      expect(roles).toHaveBeenCalledWith(2, 'admin');
      expect(save).not.toHaveBeenCalled();
      expect(useAccountsStore.getState().roleDraft).toEqual({});
    });

    it('saves settings and roles together from one press', async () => {
      const save = acceptSettings();
      const roles = vi.spyOn(api, 'updateUserRole').mockResolvedValue(account(2, 'admin'));
      useSettingsStore.setState({ draft: { segmentSeconds: 30 } });
      useAccountsStore.setState({ roleDraft: { 2: 'admin' } });
      const user = renderBar();
      await user.click(saveButton());

      expect(save).toHaveBeenCalledWith({ segmentSeconds: 30 });
      expect(roles).toHaveBeenCalledWith(2, 'admin');
    });

    it('says Saving and holds every button while the save is in flight', async () => {
      let answer: (settings: Settings) => void = () => undefined;
      vi.spyOn(api, 'updateSettings').mockImplementation(
        () => new Promise<Settings>((resolve) => (answer = resolve)),
      );
      useSettingsStore.setState({ settings: { ...STORED, gain: 3 }, draft: { gain: 2 } });
      const user = renderBar();
      await user.click(saveButton());

      expect(saveButton()).toHaveTextContent('Saving...');
      expect(saveButton()).toBeDisabled();
      expect(discardButton()).toBeDisabled();
      expect(restoreButton()).toBeDisabled();

      await act(async () => {
        answer({ ...STORED, gain: 2 });
      });
      expect(saveButton()).toHaveTextContent('Save changes');
    });

    it('shows why the server refused the settings and keeps them pending', async () => {
      vi.spyOn(api, 'updateSettings').mockRejectedValue(
        new ApiError('the recording directory cannot be written to', 'bad_request', 400),
      );
      useSettingsStore.setState({ draft: { recordingsDir: '/root' } });
      const user = renderBar();
      await user.click(saveButton());

      expect(screen.getByText('the recording directory cannot be written to')).toBeInTheDocument();
      expect(screen.getByText('1 unsaved change')).toBeInTheDocument();
    });

    it('says a refused role change is still marked unsaved, with the reason', async () => {
      vi.spyOn(api, 'updateUserRole').mockRejectedValue(
        new ApiError('there must always be at least one admin', 'conflict', 409),
      );
      useAccountsStore.setState({ users: [account(1, 'admin'), account(2, 'admin')], roleDraft: { 2: 'listener' } });
      vi.spyOn(api, 'users').mockResolvedValue([account(1, 'admin'), account(2, 'admin')]);
      const user = renderBar();
      await user.click(saveButton());

      expect(
        screen.getByText(
          'A role change was not saved: there must always be at least one admin. It is still marked unsaved under Access.',
        ),
      ).toBeInTheDocument();
      expect(screen.getByText('1 unsaved change')).toBeInTheDocument();
    });
  });

  describe('a save that shortens retention', () => {
    it('asks before saving, naming the old and new windows', async () => {
      const save = acceptSettings();
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 168 }, draft: { retentionHours: 24 } });
      const user = renderBar();
      await user.click(saveButton());

      expect(save).not.toHaveBeenCalled();
      expect(screen.getByText('Save and delete older audio?')).toBeInTheDocument();
      expect(
        screen.getByText(/Retention goes from 7 days to 24 hours\. Anything older is deleted within a minute/),
      ).toBeInTheDocument();
    });

    it('saves once the deletion is confirmed', async () => {
      const save = acceptSettings();
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 168 }, draft: { retentionHours: 24 } });
      const user = renderBar();
      await user.click(saveButton());
      await user.click(screen.getByRole('button', { name: 'Save and delete' }));

      expect(save).toHaveBeenCalledWith({ retentionHours: 24 });
      expect(screen.queryByText('Save and delete older audio?')).not.toBeInTheDocument();
    });

    it('saves nothing when the question is cancelled', async () => {
      const save = acceptSettings();
      useSettingsStore.setState({ settings: { ...STORED, retentionHours: 168 }, draft: { retentionHours: 24 } });
      const user = renderBar();
      await user.click(saveButton());
      await user.click(screen.getByRole('button', { name: 'Cancel' }));

      expect(save).not.toHaveBeenCalled();
      expect(screen.queryByText('Save and delete older audio?')).not.toBeInTheDocument();
      expect(useSettingsStore.getState().draft).toEqual({ retentionHours: 24 });
    });

    it('asks when leaving keep forever, however long the new window', async () => {
      useSettingsStore.setState({
        settings: { ...STORED, retentionHours: null },
        draft: { retentionHours: 8760 },
      });
      const user = renderBar();
      await user.click(saveButton());
      expect(screen.getByText(/Retention goes from forever to 365 days/)).toBeInTheDocument();
    });

    it('saves at once when the window grows', async () => {
      const save = acceptSettings();
      useSettingsStore.setState({ draft: { retentionHours: 168 } });
      const user = renderBar();
      await user.click(saveButton());
      expect(save).toHaveBeenCalledWith({ retentionHours: 168 });
      expect(screen.queryByText('Save and delete older audio?')).not.toBeInTheDocument();
    });

    it('saves at once when switching to keep forever, which deletes nothing', async () => {
      const save = acceptSettings();
      useSettingsStore.setState({ draft: { retentionHours: null } });
      const user = renderBar();
      await user.click(saveButton());
      expect(save).toHaveBeenCalledWith({ retentionHours: null });
    });
  });
});
