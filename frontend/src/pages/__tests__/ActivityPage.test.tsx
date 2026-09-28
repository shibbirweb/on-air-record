// @vitest-environment jsdom

/**
 * The activity page: the log, newest first, with who did what, when, and from where. Admins only; a
 * listener who types the address is sent back, and the server refuses them regardless. The store's request
 * actions are replaced with spies, so these test what the page shows and asks for.
 */

import '@/test/dom';

import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { ActivityEntry } from '@/api/types';
import { account } from '@/features/settings/__tests__/fixtures';
import { formatDateTime } from '@/lib/format';
import { useAccountsStore } from '@/store/useAccountsStore';
import { useActivityStore } from '@/store/useActivityStore';
import { useAuthStore } from '@/store/useAuthStore';

import { ActivityPage } from '../ActivityPage';

const ADMIN = account(1, 'admin');
const KITCHEN = account(2, 'listener');

let refresh: Mock<() => Promise<void>>;
let loadMore: Mock<() => Promise<void>>;
let setFilter: Mock<(change: object) => Promise<void>>;

const SIGNED_IN: ActivityEntry = {
  id: 3,
  atMs: 1_790_000_300_000,
  actor: { kind: 'account', userId: 2, email: KITCHEN.email },
  address: '192.168.1.20',
  userAgent: 'Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X)',
  event: { kind: 'signed_in', method: 'password' },
};

const LISTENED: ActivityEntry = {
  id: 2,
  atMs: 1_790_000_200_000,
  actor: { kind: 'guest' },
  address: '192.168.1.31',
  userAgent: null,
  event: {
    kind: 'listened',
    startedAtMs: 1_790_000_000_000,
    connectedMs: 120_000,
    playedMs: 120_000,
    playedBack: false,
    earliestMs: null,
  },
};

const BY_HOST: ActivityEntry = {
  id: 1,
  atMs: 1_790_000_100_000,
  actor: { kind: 'host' },
  address: null,
  userAgent: null,
  event: { kind: 'accounts_disabled' },
};

function renderAt(path = '/activity') {
  const user = userEvent.setup();
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/" element={<p>The control room</p>} />
        <Route path="/activity" element={<ActivityPage />} />
      </Routes>
    </MemoryRouter>,
  );
  return user;
}

const rows = () => screen.getAllByRole('listitem');

describe('the activity page', () => {
  beforeEach(() => {
    refresh = vi.fn(async () => undefined);
    loadMore = vi.fn(async () => undefined);
    setFilter = vi.fn(async () => undefined);
    useAuthStore.setState({ mode: 'accounts', user: ADMIN });
    useAccountsStore.setState({ users: [ADMIN, KITCHEN], refresh: vi.fn(async () => undefined) });
    useActivityStore.setState({
      entries: [SIGNED_IN, LISTENED, BY_HOST],
      filter: { email: '', group: null },
      loaded: true,
      loading: false,
      loadingMore: false,
      hasMore: false,
      error: null,
      refresh,
      loadMore,
      setFilter,
    });
  });

  describe('who may see it', () => {
    it('sends a listener back to the control room', () => {
      useAuthStore.setState({ mode: 'accounts', user: KITCHEN });
      renderAt();
      expect(screen.getByText('The control room')).toBeInTheDocument();
      expect(refresh).not.toHaveBeenCalled();
    });

    it('is shown to an admin, and asks for the log', () => {
      renderAt();
      expect(screen.getByRole('heading', { name: 'Activity' })).toBeInTheDocument();
      expect(refresh).toHaveBeenCalledTimes(1);
    });

    it('is shown to anyone on an open recorder', () => {
      useAuthStore.setState({ mode: 'open', user: null });
      renderAt();
      expect(screen.getByRole('heading', { name: 'Activity' })).toBeInTheDocument();
    });
  });

  describe('each entry', () => {
    it('says who, what and when, newest first', () => {
      renderAt();
      const [first, second, third] = rows();
      expect(first).toHaveTextContent(KITCHEN.email);
      expect(first).toHaveTextContent('Signed in');
      expect(first).toHaveTextContent(formatDateTime(SIGNED_IN.atMs));
      expect(second).toHaveTextContent('A guest');
      expect(second).toHaveTextContent('Listened live for 2 min');
      expect(third).toHaveTextContent('The host command line');
      expect(third).toHaveTextContent('Turned sign in off and removed every account');
    });

    it('says where it came from and the browser, when known', () => {
      renderAt();
      const [first, , third] = rows();
      expect(first).toHaveTextContent('192.168.1.20');
      expect(within(first).getByTitle(SIGNED_IN.userAgent ?? '')).toBeInTheDocument();
      expect(third).not.toHaveTextContent('192.168');
    });
  });

  describe('filters', () => {
    it('narrow to one heading of events', async () => {
      const user = renderAt();
      await user.click(screen.getByRole('radio', { name: 'Listening' }));
      expect(setFilter).toHaveBeenCalledWith({ group: 'listening' });
    });

    it('go back to everything', async () => {
      useActivityStore.setState({ filter: { email: '', group: 'listening' } });
      const user = renderAt();
      await user.click(screen.getByRole('radio', { name: 'Everything' }));
      expect(setFilter).toHaveBeenCalledWith({ group: null });
    });

    it('show which heading is chosen', () => {
      useActivityStore.setState({ filter: { email: '', group: 'access' } });
      renderAt();
      expect(screen.getByRole('radio', { name: 'Sign ins' })).toBeChecked();
      expect(screen.getByRole('radio', { name: 'Everything' })).not.toBeChecked();
    });

    it('narrow to one person when an email is applied', async () => {
      const user = renderAt();
      await user.type(screen.getByLabelText('Only this person'), `${KITCHEN.email}{Enter}`);
      expect(setFilter).toHaveBeenCalledWith({ email: KITCHEN.email });
    });

    it('offer the accounts there are as suggestions', () => {
      renderAt();
      const options = [...document.querySelectorAll('datalist option')].map((option) => option.getAttribute('value'));
      expect(options).toEqual([ADMIN.email, KITCHEN.email]);
    });
  });

  describe('paging', () => {
    it('offers more when there is more', async () => {
      useActivityStore.setState({ hasMore: true });
      const user = renderAt();
      await user.click(screen.getByRole('button', { name: 'Load older entries' }));
      expect(loadMore).toHaveBeenCalledTimes(1);
    });

    it('offers nothing more at the end', () => {
      renderAt();
      expect(screen.queryByRole('button', { name: 'Load older entries' })).not.toBeInTheDocument();
      expect(screen.getByText('That is everything kept.')).toBeInTheDocument();
    });

    it('waits while the next page is on its way', () => {
      useActivityStore.setState({ hasMore: true, loadingMore: true });
      renderAt();
      expect(screen.getByRole('button', { name: 'Load older entries' })).toBeDisabled();
    });
  });

  describe('when there is nothing to show', () => {
    it('says the log is empty', () => {
      useActivityStore.setState({ entries: [] });
      renderAt();
      expect(screen.getByText('Nothing has been logged yet.')).toBeInTheDocument();
    });

    it('says nothing matches when a filter is on', () => {
      useActivityStore.setState({ entries: [], filter: { email: KITCHEN.email, group: null } });
      renderAt();
      expect(screen.getByText('Nothing matches these filters.')).toBeInTheDocument();
    });

    it('says it is loading before the first answer', () => {
      useActivityStore.setState({ entries: [], loaded: false, loading: true });
      renderAt();
      expect(screen.getByText('Loading the activity log...')).toBeInTheDocument();
      expect(screen.queryByText('Nothing has been logged yet.')).not.toBeInTheDocument();
    });
  });

  it('shows what went wrong', () => {
    useActivityStore.setState({ error: 'database is locked' });
    renderAt();
    expect(screen.getByText('database is locked')).toBeInTheDocument();
  });
});
