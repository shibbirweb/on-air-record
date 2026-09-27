// @vitest-environment jsdom

/**
 * The Updates card: the running version and channel, what the last check found, Check now, and the switch
 * for the automatic check. The switch is the service's only request to the internet, so it matters that it
 * shows what is stored or pending and that flipping it only stages a change for Save. Check now is not a
 * setting: pressing it is the request. The update store's two requests are replaced with spies and the
 * real settings store holds the draft.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { updateStatus, upToDate } from '@/features/updates/__tests__/fixtures';
import { formatDateTime } from '@/lib/format';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useUpdateStore } from '@/store/useUpdateStore';

import { UpdateSettings } from '../UpdateSettings';
import { STORED } from './fixtures';

let refresh: Mock<() => Promise<void>>;
let checkNow: Mock<() => Promise<void>>;

const automaticSwitch = () => screen.getByRole('switch', { name: 'Check for updates automatically' });
const checkNowButton = () => screen.getByRole('button', { name: 'Check now' });

function renderCard() {
  const user = userEvent.setup();
  render(<UpdateSettings />);
  return user;
}

describe('the updates card', () => {
  beforeEach(() => {
    refresh = vi.fn(async () => undefined);
    checkNow = vi.fn(async () => undefined);
    useSettingsStore.setState({ settings: STORED, defaults: STORED, draft: {}, error: null });
    useUpdateStore.setState({ status: upToDate(), checking: false, requestError: null, refresh, checkNow });
  });

  describe('what it reports', () => {
    it('asks the service for its latest answer when shown', () => {
      render(<UpdateSettings />);
      expect(refresh).toHaveBeenCalledTimes(1);
      expect(checkNow).not.toHaveBeenCalled();
    });

    it('shows the running version and that it follows stable releases', () => {
      render(<UpdateSettings />);
      expect(screen.getByText(/Version/)).toHaveTextContent('Version 0.8.1, following stable releases.');
    });

    it('says a beta install follows beta and stable releases', () => {
      useUpdateStore.setState({ status: upToDate({ channel: 'beta', currentVersion: '0.9.0-beta.1' }) });
      render(<UpdateSettings />);
      expect(screen.getByText(/Version/)).toHaveTextContent(
        'Version 0.9.0-beta.1, following beta and stable releases.',
      );
    });

    it('holds a place for the version and says nothing was checked before the service answers', () => {
      useUpdateStore.setState({ status: null });
      render(<UpdateSettings />);
      expect(screen.getByText(/Version/)).toHaveTextContent('Version ..., following stable releases.');
      expect(screen.getByText('Not checked yet.')).toBeInTheDocument();
      expect(screen.queryByText('Up to date.')).not.toBeInTheDocument();
    });

    it('says it is up to date, and when it last checked', () => {
      const status = upToDate();
      useUpdateStore.setState({ status });
      render(<UpdateSettings />);
      expect(screen.getByText('Up to date.')).toBeInTheDocument();
      expect(screen.getByText(`Last checked ${formatDateTime(status.checkedAtMs)}.`)).toBeInTheDocument();
    });

    it('does not claim to be up to date before any check has run', () => {
      useUpdateStore.setState({ status: upToDate({ checkedAtMs: null }) });
      render(<UpdateSettings />);
      expect(screen.queryByText('Up to date.')).not.toBeInTheDocument();
      expect(screen.getByText('Not checked yet.')).toBeInTheDocument();
    });

    it('names a newer release and offers what is new', async () => {
      useUpdateStore.setState({ status: updateStatus() });
      const user = renderCard();
      expect(screen.getByText('0.9.0 is available.')).toBeInTheDocument();
      expect(screen.queryByText('Up to date.')).not.toBeInTheDocument();

      await user.click(screen.getByRole('button', { name: "What's new" }));
      expect(screen.getByRole('dialog', { name: "What's new in 0.9.0" })).toBeInTheDocument();
    });

    it('offers no what is new when nothing newer is out', () => {
      render(<UpdateSettings />);
      expect(screen.queryByRole('button', { name: "What's new" })).not.toBeInTheDocument();
    });

    it('says why the last check failed', () => {
      useUpdateStore.setState({ status: upToDate({ error: 'could not reach GitHub' }) });
      render(<UpdateSettings />);
      expect(screen.getByText('The last check failed: could not reach GitHub.')).toBeInTheDocument();
    });

    it('says when the service itself could not be asked', () => {
      useUpdateStore.setState({ requestError: 'could not ask the service about updates' });
      render(<UpdateSettings />);
      expect(screen.getByText('could not ask the service about updates')).toBeInTheDocument();
    });
  });

  describe('Check now', () => {
    it('asks straight away', async () => {
      const user = renderCard();
      await user.click(checkNowButton());
      expect(checkNow).toHaveBeenCalledTimes(1);
    });

    it('cannot be pressed again while a check is running', () => {
      useUpdateStore.setState({ checking: true });
      render(<UpdateSettings />);
      expect(checkNowButton()).toBeDisabled();
    });

    it('shows what the check found once it answers', async () => {
      checkNow.mockImplementation(async () => {
        useUpdateStore.setState({ status: updateStatus() });
      });
      const user = renderCard();
      await user.click(checkNowButton());
      await act(async () => {
        await Promise.resolve();
      });
      expect(screen.getByText('0.9.0 is available.')).toBeInTheDocument();
    });

    it('stages nothing in the settings draft', async () => {
      const user = renderCard();
      await user.click(checkNowButton());
      expect(useSettingsStore.getState().draft).toEqual({});
    });
  });

  describe('the automatic check switch', () => {
    it('is on when the stored setting is on', () => {
      render(<UpdateSettings />);
      expect(automaticSwitch()).toBeChecked();
    });

    it('is off when the stored setting is off', () => {
      useSettingsStore.setState({ settings: { ...STORED, checkForUpdates: false } });
      render(<UpdateSettings />);
      expect(automaticSwitch()).not.toBeChecked();
    });

    it('shows a pending change over the stored setting', () => {
      useSettingsStore.setState({ draft: { checkForUpdates: false } });
      render(<UpdateSettings />);
      expect(automaticSwitch()).not.toBeChecked();
    });

    it('starts on before the settings have loaded', () => {
      useSettingsStore.setState({ settings: null });
      render(<UpdateSettings />);
      expect(automaticSwitch()).toBeChecked();
    });

    it('stages switching off in the draft, to wait for Save', async () => {
      const user = renderCard();
      await user.click(automaticSwitch());
      expect(useSettingsStore.getState().draft).toEqual({ checkForUpdates: false });
      expect(automaticSwitch()).not.toBeChecked();
    });

    it('forgets the change when switched back', async () => {
      const user = renderCard();
      await user.click(automaticSwitch());
      await user.click(automaticSwitch());
      expect(useSettingsStore.getState().draft).toEqual({});
    });

    it('says it is the only request the recorder makes to the internet, and never installs anything', () => {
      render(<UpdateSettings />);
      expect(screen.getByText(/This is the only request the recorder makes to the internet/)).toBeInTheDocument();
      expect(screen.getByText(/It never\s+installs anything/)).toBeInTheDocument();
    });
  });
});
