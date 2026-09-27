// @vitest-environment jsdom

/**
 * The Access card. On an open recorder it offers to switch accounts on; with accounts it lists everyone
 * and lets an admin add, remove and reset them. Role changes are staged for the Save bar while the other
 * actions happen as soon as they are confirmed, and your own row can never be changed from here, which is
 * what keeps the last admin in place. The real accounts store is used with the server calls replaced by
 * spies, so each test proves both what is sent and what the list shows after the store reloads it.
 */

import '@/test/dom';

import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { api, ApiError } from '@/api/client';
import type { Role, User } from '@/api/types';
import { useAccountsStore } from '@/store/useAccountsStore';
import { useAuthStore } from '@/store/useAuthStore';

import { AccessSettings } from '../AccessSettings';
import { account } from './fixtures';

const ME = account(1, 'admin');

/** What the server holds; the users spy answers from it, so a reload shows the latest state. */
let serverUsers: User[] = [];
let setUp: Mock<(email: string, password: string) => Promise<void>>;

/** The list item for one account. */
function row(email: string): HTMLElement {
  const item = screen.getByText(email).closest('li');
  if (!item) {
    throw new Error(`no row for ${email}`);
  }
  return item;
}

function renderAccounts() {
  const user = userEvent.setup();
  render(<AccessSettings />);
  return user;
}

/** Render with accounts on and wait for the list to arrive from the server. */
async function renderList() {
  const user = renderAccounts();
  await screen.findByText('person2@example.com');
  return user;
}

describe('the access card on an open recorder', () => {
  beforeEach(() => {
    setUp = vi.fn(async () => undefined);
    useAuthStore.setState({ mode: 'open', user: null, setUp });
  });

  it('says anyone can do anything and offers to set up accounts', () => {
    render(<AccessSettings />);
    expect(screen.getByText(/No login is required/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Set up accounts' })).toBeInTheDocument();
  });

  it('treats a recorder nobody has decided about yet as open', () => {
    useAuthStore.setState({ mode: 'undecided' });
    render(<AccessSettings />);
    expect(screen.getByRole('button', { name: 'Set up accounts' })).toBeInTheDocument();
  });

  it('creates the admin account from the dialog and closes it', async () => {
    const user = renderAccounts();
    await user.click(screen.getByRole('button', { name: 'Set up accounts' }));
    const dialog = screen.getByRole('dialog', { name: 'Create the admin account' });

    await user.type(within(dialog).getByLabelText('Email'), ' admin@example.com ');
    await user.type(within(dialog).getByLabelText('Password'), 'a long passphrase');
    await user.type(within(dialog).getByLabelText('Password again'), 'a long passphrase');
    await user.click(within(dialog).getByRole('button', { name: 'Create and sign in' }));

    expect(setUp).toHaveBeenCalledWith('admin@example.com', 'a long passphrase');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('keeps the dialog open with the reason when the server refuses', async () => {
    setUp.mockRejectedValue(new ApiError('accounts are already set up', 'conflict', 409));
    const user = renderAccounts();
    await user.click(screen.getByRole('button', { name: 'Set up accounts' }));
    const dialog = screen.getByRole('dialog');

    await user.type(within(dialog).getByLabelText('Email'), 'admin@example.com');
    await user.type(within(dialog).getByLabelText('Password'), 'a long passphrase');
    await user.type(within(dialog).getByLabelText('Password again'), 'a long passphrase');
    await user.click(within(dialog).getByRole('button', { name: 'Create and sign in' }));

    expect(within(dialog).getByRole('alert')).toHaveTextContent('Accounts are already set up');
  });

  it('checks the password before asking the server', async () => {
    const user = renderAccounts();
    await user.click(screen.getByRole('button', { name: 'Set up accounts' }));
    const dialog = screen.getByRole('dialog');

    await user.type(within(dialog).getByLabelText('Email'), 'admin@example.com');
    await user.type(within(dialog).getByLabelText('Password'), 'short');
    await user.type(within(dialog).getByLabelText('Password again'), 'short');
    await user.click(within(dialog).getByRole('button', { name: 'Create and sign in' }));

    expect(within(dialog).getByRole('alert')).toHaveTextContent('Use at least 8 characters.');
    expect(setUp).not.toHaveBeenCalled();
  });
});

describe('the access card with accounts', () => {
  beforeEach(() => {
    serverUsers = [ME, account(2, 'listener'), account(3, 'admin', true)];
    useAuthStore.setState({ mode: 'accounts', user: ME });
    useAccountsStore.setState({
      users: [],
      loading: false,
      error: null,
      roleDraft: {},
      savingRoles: false,
      roleError: null,
    });
    vi.spyOn(api, 'users').mockImplementation(async () => serverUsers.map((entry) => ({ ...entry })));
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('the list', () => {
    it('loads the accounts from the server when shown', async () => {
      await renderList();
      expect(api.users).toHaveBeenCalled();
      expect(screen.getByText('person1@example.com')).toBeInTheDocument();
      expect(screen.getByText('person3@example.com')).toBeInTheDocument();
    });

    it('says it is loading while the first answer is on its way', () => {
      vi.spyOn(api, 'users').mockImplementation(() => new Promise<User[]>(() => undefined));
      render(<AccessSettings />);
      expect(screen.getByText('Loading accounts...')).toBeInTheDocument();
    });

    it('says when there are no accounts', async () => {
      serverUsers = [];
      render(<AccessSettings />);
      expect(await screen.findByText('No accounts.')).toBeInTheDocument();
    });

    it('marks your own account, and shows its role without a menu or buttons', async () => {
      await renderList();
      const mine = row('person1@example.com');
      expect(within(mine).getByText('you')).toBeInTheDocument();
      expect(within(mine).getByText('Admin')).toBeInTheDocument();
      expect(within(mine).queryByRole('combobox')).not.toBeInTheDocument();
      expect(within(mine).queryByRole('button')).not.toBeInTheDocument();
    });

    it('gives every other account a role menu, a password button and a remove button', async () => {
      await renderList();
      const other = row('person2@example.com');
      expect(within(other).getByRole('combobox')).toHaveTextContent('Listener');
      expect(
        within(other).getByRole('button', { name: 'Set a new password for person2@example.com' }),
      ).toBeInTheDocument();
      expect(within(other).getByRole('button', { name: 'Remove person2@example.com' })).toBeInTheDocument();
      expect(within(other).queryByText('you')).not.toBeInTheDocument();
    });

    it('marks accounts that sign in with two factor, and only those', async () => {
      await renderList();
      expect(within(row('person3@example.com')).getByText('2FA')).toBeInTheDocument();
      expect(within(row('person2@example.com')).queryByText('2FA')).not.toBeInTheDocument();
    });

    it('shows what went wrong above the list, capitalised', async () => {
      vi.spyOn(api, 'users').mockRejectedValue(new ApiError('could not read the accounts', 'internal', 500));
      render(<AccessSettings />);
      expect(await screen.findByRole('alert')).toHaveTextContent('Could not read the accounts');
    });
  });

  describe('changing a role', () => {
    it('stages the new role and marks the account unsaved, sending nothing', async () => {
      const update = vi.spyOn(api, 'updateUserRole');
      const user = await renderList();
      const other = row('person2@example.com');
      await user.click(within(other).getByRole('combobox'));
      await user.click(screen.getByRole('option', { name: 'Admin' }));

      expect(useAccountsStore.getState().roleDraft).toEqual({ 2: 'admin' });
      expect(within(other).getByRole('combobox')).toHaveTextContent('Admin');
      expect(within(other).getByText('Unsaved')).toBeInTheDocument();
      expect(update).not.toHaveBeenCalled();
    });

    it('takes the account out of the draft when its old role is picked again', async () => {
      const user = await renderList();
      const other = row('person2@example.com');
      await user.click(within(other).getByRole('combobox'));
      await user.click(screen.getByRole('option', { name: 'Admin' }));
      await user.click(within(other).getByRole('combobox'));
      await user.click(screen.getByRole('option', { name: 'Listener' }));

      expect(useAccountsStore.getState().roleDraft).toEqual({});
      expect(within(other).queryByText('Unsaved')).not.toBeInTheDocument();
    });

    it('explains an unsaved mark by the role the account has now', async () => {
      useAccountsStore.setState({ roleDraft: { 3: 'listener' } });
      await renderList();
      expect(within(row('person3@example.com')).getByText('Unsaved')).toHaveAttribute(
        'title',
        'Now an admin. Save changes to apply.',
      );
    });

    it('puts every menu back when the draft is discarded elsewhere', async () => {
      useAccountsStore.setState({ roleDraft: { 2: 'admin' } });
      await renderList();
      expect(within(row('person2@example.com')).getByRole('combobox')).toHaveTextContent('Admin');

      useAccountsStore.getState().discardRoles();
      await waitFor(() => {
        expect(within(row('person2@example.com')).getByRole('combobox')).toHaveTextContent('Listener');
      });
    });
  });

  describe('removing an account', () => {
    it('asks first, and does nothing when kept', async () => {
      const remove = vi.spyOn(api, 'deleteUser');
      const user = await renderList();
      const other = row('person2@example.com');
      await user.click(within(other).getByRole('button', { name: 'Remove person2@example.com' }));
      await user.click(within(other).getByRole('button', { name: 'Keep' }));

      expect(remove).not.toHaveBeenCalled();
      expect(within(other).getByRole('button', { name: 'Remove person2@example.com' })).toBeInTheDocument();
    });

    it('removes the account once confirmed and reloads the list', async () => {
      const remove = vi.spyOn(api, 'deleteUser').mockImplementation(async (userId: number) => {
        serverUsers = serverUsers.filter((entry) => entry.id !== userId);
      });
      const user = await renderList();
      const other = row('person2@example.com');
      await user.click(within(other).getByRole('button', { name: 'Remove person2@example.com' }));
      await user.click(within(other).getByRole('button', { name: 'Remove' }));

      expect(remove).toHaveBeenCalledWith(2);
      await waitFor(() => {
        expect(screen.queryByText('person2@example.com')).not.toBeInTheDocument();
      });
    });
  });

  describe('removing two factor sign in', () => {
    it('is offered only for accounts that use it', async () => {
      await renderList();
      expect(
        within(row('person3@example.com')).getByRole('button', {
          name: 'Remove two factor sign in for person3@example.com',
        }),
      ).toBeInTheDocument();
      expect(
        within(row('person2@example.com')).queryByRole('button', { name: /two factor/ }),
      ).not.toBeInTheDocument();
    });

    it('asks first, and does nothing when kept', async () => {
      const reset = vi.spyOn(api, 'resetUserTwoFactor');
      const user = await renderList();
      const other = row('person3@example.com');
      await user.click(within(other).getByRole('button', { name: /Remove two factor sign in/ }));
      await user.click(within(other).getByRole('button', { name: 'Keep' }));

      expect(reset).not.toHaveBeenCalled();
      expect(within(other).getByRole('button', { name: /Remove two factor sign in/ })).toBeInTheDocument();
    });

    it('removes it once confirmed, and the badge goes with it', async () => {
      const reset = vi.spyOn(api, 'resetUserTwoFactor').mockImplementation(async (userId: number) => {
        serverUsers = serverUsers.map((entry) =>
          entry.id === userId ? { ...entry, twoFactorEnabled: false } : entry,
        );
      });
      const user = await renderList();
      const other = row('person3@example.com');
      await user.click(within(other).getByRole('button', { name: /Remove two factor sign in/ }));
      await user.click(within(other).getByRole('button', { name: 'Remove 2FA' }));

      expect(reset).toHaveBeenCalledWith(3);
      await waitFor(() => {
        expect(within(row('person3@example.com')).queryByText('2FA')).not.toBeInTheDocument();
      });
    });
  });

  describe('setting a password', () => {
    async function openPasswordDialog() {
      const user = await renderList();
      await user.click(screen.getByRole('button', { name: 'Set a new password for person2@example.com' }));
      const dialog = screen.getByRole('dialog', { name: 'New password for person2@example.com' });
      return { user, dialog };
    }

    it('sets the password and closes once both fields agree', async () => {
      const setPassword = vi.spyOn(api, 'setUserPassword').mockResolvedValue(undefined);
      const { user, dialog } = await openPasswordDialog();
      await user.type(within(dialog).getByLabelText('New password'), 'another passphrase');
      await user.type(within(dialog).getByLabelText('New password again'), 'another passphrase');
      await user.click(within(dialog).getByRole('button', { name: 'Set password' }));

      expect(setPassword).toHaveBeenCalledWith(2, 'another passphrase');
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });

    it('refuses a short password without asking the server', async () => {
      const setPassword = vi.spyOn(api, 'setUserPassword');
      const { user, dialog } = await openPasswordDialog();
      await user.type(within(dialog).getByLabelText('New password'), 'short');
      await user.type(within(dialog).getByLabelText('New password again'), 'short');
      await user.click(within(dialog).getByRole('button', { name: 'Set password' }));

      expect(within(dialog).getByRole('alert')).toHaveTextContent('Use at least 8 characters.');
      expect(setPassword).not.toHaveBeenCalled();
    });

    it('refuses two passwords that differ', async () => {
      const setPassword = vi.spyOn(api, 'setUserPassword');
      const { user, dialog } = await openPasswordDialog();
      await user.type(within(dialog).getByLabelText('New password'), 'another passphrase');
      await user.type(within(dialog).getByLabelText('New password again'), 'another passphrasf');
      await user.click(within(dialog).getByRole('button', { name: 'Set password' }));

      expect(within(dialog).getByRole('alert')).toHaveTextContent('The two passwords do not match.');
      expect(setPassword).not.toHaveBeenCalled();
    });

    it('stays open with the server reason when it refuses', async () => {
      vi.spyOn(api, 'setUserPassword').mockRejectedValue(
        new ApiError('the account no longer exists', 'not_found', 404),
      );
      const { user, dialog } = await openPasswordDialog();
      await user.type(within(dialog).getByLabelText('New password'), 'another passphrase');
      await user.type(within(dialog).getByLabelText('New password again'), 'another passphrase');
      await user.click(within(dialog).getByRole('button', { name: 'Set password' }));

      expect(within(dialog).getByRole('alert')).toHaveTextContent('The account no longer exists');
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });

    it('forgets what was typed when cancelled', async () => {
      const { user, dialog } = await openPasswordDialog();
      await user.type(within(dialog).getByLabelText('New password'), 'half typed');
      await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

      await user.click(screen.getByRole('button', { name: 'Set a new password for person2@example.com' }));
      expect(screen.getByLabelText('New password')).toHaveValue('');
    });
  });

  describe('adding an account', () => {
    async function openAddDialog() {
      const user = await renderList();
      await user.click(screen.getByRole('button', { name: 'Add an account' }));
      const dialog = screen.getByRole('dialog', { name: 'Add an account' });
      return { user, dialog };
    }

    async function fillIn(user: ReturnType<typeof userEvent.setup>, dialog: HTMLElement, email: string) {
      await user.type(within(dialog).getByLabelText('Email'), email);
      await user.type(within(dialog).getByLabelText('Password'), 'first passphrase');
      await user.type(within(dialog).getByLabelText('Password again'), 'first passphrase');
    }

    it('adds a listener by default, then closes and shows the new account', async () => {
      const createUser = vi
        .spyOn(api, 'createUser')
        .mockImplementation(async (email: string, _password: string, role: Role) => {
          const created: User = { id: 4, email, role, createdAtMs: 0, twoFactorEnabled: false };
          serverUsers = [...serverUsers, created];
          return created;
        });
      const { user, dialog } = await openAddDialog();
      expect(within(dialog).getByRole('combobox')).toHaveTextContent('Listener');
      await fillIn(user, dialog, 'kitchen@example.com');
      await user.click(within(dialog).getByRole('button', { name: 'Add account' }));

      expect(createUser).toHaveBeenCalledWith('kitchen@example.com', 'first passphrase', 'listener');
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
      expect(await screen.findByText('kitchen@example.com')).toBeInTheDocument();
    });

    it('adds an admin when Admin is picked', async () => {
      const createUser = vi.spyOn(api, 'createUser').mockResolvedValue(account(4, 'admin'));
      const { user, dialog } = await openAddDialog();
      await fillIn(user, dialog, 'deputy@example.com');
      await user.click(within(dialog).getByRole('combobox'));
      await user.click(screen.getByRole('option', { name: 'Admin' }));
      await user.click(within(dialog).getByRole('button', { name: 'Add account' }));

      expect(createUser).toHaveBeenCalledWith('deputy@example.com', 'first passphrase', 'admin');
    });

    it('stays open with the server reason when it refuses', async () => {
      vi.spyOn(api, 'createUser').mockRejectedValue(
        new ApiError('an account with that email already exists', 'conflict', 409),
      );
      const { user, dialog } = await openAddDialog();
      await fillIn(user, dialog, 'person2@example.com');
      await user.click(within(dialog).getByRole('button', { name: 'Add account' }));

      expect(within(dialog).getByRole('alert')).toHaveTextContent('An account with that email already exists');
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });

    it('checks the email before asking the server', async () => {
      const createUser = vi.spyOn(api, 'createUser');
      const { user, dialog } = await openAddDialog();
      await fillIn(user, dialog, 'not an email');
      await user.click(within(dialog).getByRole('button', { name: 'Add account' }));

      expect(within(dialog).getByRole('alert')).toHaveTextContent('Enter an email address.');
      expect(createUser).not.toHaveBeenCalled();
    });

    it('starts the next account from empty fields', async () => {
      vi.spyOn(api, 'createUser').mockResolvedValue(account(4, 'listener'));
      const { user, dialog } = await openAddDialog();
      await fillIn(user, dialog, 'kitchen@example.com');
      await user.click(within(dialog).getByRole('button', { name: 'Add account' }));

      await user.click(screen.getByRole('button', { name: 'Add an account' }));
      expect(within(screen.getByRole('dialog')).getByLabelText('Email')).toHaveValue('');
    });
  });
});
