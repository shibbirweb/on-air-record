// @vitest-environment jsdom

/**
 * The settings page as a whole: who may open it, what it fetches on arrival, and that an edit in any card
 * reaches the one Save bar at the foot of the page. Listeners are sent back to the control room even when
 * they type the address, because the link being hidden is not a rule. The stores' network actions are
 * replaced with spies; each card's own behaviour is tested beside the card.
 */

import '@/test/dom';

import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { account, serviceStatus, storageUsage, STORED } from '@/features/settings/__tests__/fixtures';
import { upToDate } from '@/features/updates/__tests__/fixtures';
import { useAccountsStore } from '@/store/useAccountsStore';
import { useAuthStore } from '@/store/useAuthStore';
import { useMetricsStore } from '@/store/useMetricsStore';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useStatusStore } from '@/store/useStatusStore';
import { useStorageStore } from '@/store/useStorageStore';
import { useUpdateStore } from '@/store/useUpdateStore';

import { SettingsPage } from '../SettingsPage';

const ADMIN = account(1, 'admin');
const LISTENER = account(2, 'listener');

let refreshSettings: Mock<() => Promise<void>>;
let refreshStorage: Mock<() => Promise<void>>;

function renderAt(path = '/settings') {
  const user = userEvent.setup();
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/" element={<p>The control room</p>} />
        <Route path="/settings" element={<SettingsPage />} />
      </Routes>
    </MemoryRouter>,
  );
  return user;
}

const CARDS = [
  'History and storage',
  'Recording location',
  'Audio',
  'Finding sounds',
  'Updates',
  'Access',
  'Monitoring',
];

describe('the settings page', () => {
  beforeEach(() => {
    refreshSettings = vi.fn(async () => undefined);
    refreshStorage = vi.fn(async () => undefined);
    useAuthStore.setState({ mode: 'accounts', user: ADMIN });
    useSettingsStore.setState({
      settings: STORED,
      defaults: STORED,
      draft: {},
      saving: false,
      error: null,
      refresh: refreshSettings,
    });
    useStorageStore.setState({ storage: storageUsage(0, 48_000 * 2 * 3600), refresh: refreshStorage });
    useStatusStore.setState({ status: serviceStatus(48_000) });
    useUpdateStore.setState({ status: upToDate(), refresh: vi.fn(async () => undefined) });
    useMetricsStore.setState({ refresh: vi.fn(async () => undefined) });
    useAccountsStore.setState({
      users: [ADMIN, LISTENER],
      roleDraft: {},
      roleError: null,
      savingRoles: false,
      refresh: vi.fn(async () => undefined),
    });
  });

  describe('who may open it', () => {
    it('sends a listener back to the control room', () => {
      useAuthStore.setState({ user: LISTENER });
      renderAt();
      expect(screen.getByText('The control room')).toBeInTheDocument();
      expect(screen.queryByRole('heading', { name: 'Settings' })).not.toBeInTheDocument();
    });

    it('opens for an admin, with every card', () => {
      renderAt();
      expect(screen.getByRole('heading', { name: 'Settings' })).toBeInTheDocument();
      for (const title of CARDS) {
        expect(screen.getByText(title, { selector: '[data-slot="card-title"]' })).toBeInTheDocument();
      }
    });

    it('opens for anyone on a recorder without accounts', () => {
      useAuthStore.setState({ mode: 'open', user: null });
      renderAt();
      expect(screen.getByRole('heading', { name: 'Settings' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Set up accounts' })).toBeInTheDocument();
    });

    it('says edits are held until they are saved', () => {
      renderAt();
      expect(screen.getByText(/Edits are held until you save them/)).toBeInTheDocument();
    });
  });

  describe('what it fetches', () => {
    it('reloads the settings on arrival when nothing is pending', () => {
      renderAt();
      expect(refreshSettings).toHaveBeenCalledTimes(1);
    });

    it('keeps pending edits rather than reloading over them', () => {
      useSettingsStore.setState({ draft: { gain: 2 } });
      renderAt();
      expect(refreshSettings).not.toHaveBeenCalled();
      expect(screen.getByText('2.00x')).toBeInTheDocument();
      expect(screen.getByText('1 unsaved change')).toBeInTheDocument();
    });

    it('fetches disk usage straight away for the storage projection', () => {
      renderAt();
      expect(refreshStorage).toHaveBeenCalled();
    });
  });

  describe('the one Save bar', () => {
    it('counts an edit made in any card', async () => {
      const user = renderAt();
      expect(screen.getByText('Everything is at its default.')).toBeInTheDocument();

      await user.click(screen.getByRole('switch', { name: 'Record on start up' }));
      await user.click(screen.getByRole('button', { name: '7 days' }));
      expect(screen.getByText('2 unsaved changes')).toBeInTheDocument();
    });

    it('counts a staged role alongside the settings', async () => {
      const user = renderAt();
      await user.click(screen.getByRole('switch', { name: 'Check for updates automatically' }));

      const listenerRow = screen.getByText(LISTENER.email).closest('li');
      if (!listenerRow) {
        throw new Error('no row for the listener');
      }
      await user.click(within(listenerRow).getByRole('combobox'));
      await user.click(screen.getByRole('option', { name: 'Admin' }));
      expect(screen.getByText('2 unsaved changes')).toBeInTheDocument();
    });

    it('puts every card back when the edits are discarded', async () => {
      const user = renderAt();
      await user.click(screen.getByRole('switch', { name: 'Record on start up' }));
      await user.click(screen.getByRole('button', { name: 'Discard' }));

      expect(screen.getByRole('switch', { name: 'Record on start up' })).toBeChecked();
      expect(screen.getByText('Everything is at its default.')).toBeInTheDocument();
    });
  });
});
