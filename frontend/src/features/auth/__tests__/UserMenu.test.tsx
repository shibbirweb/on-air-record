// @vitest-environment jsdom

/**
 * The account menu in the header: who is signed in and their role, the way to account settings, and sign
 * out. It renders nothing without accounts. The auth store's logOut is a spy, since the real one reloads
 * the page; navigation is proven through a MemoryRouter.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { User } from '@/api/types';
import { useAuthStore } from '@/store/useAuthStore';

import { UserMenu } from '../UserMenu';

function account(role: User['role']): User {
  return { id: 1, email: `${role}@example.com`, role, createdAtMs: 0, twoFactorEnabled: false };
}

let logOut: Mock<() => Promise<void>>;

function renderMenu() {
  return render(
    <MemoryRouter initialEntries={['/']}>
      <UserMenu />
      <Routes>
        <Route path="/" element={<p>Control room page</p>} />
        <Route path="/account" element={<p>Account page</p>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe('the account menu', () => {
  beforeEach(() => {
    logOut = vi.fn(async () => undefined);
    useAuthStore.setState({ mode: 'accounts', user: account('admin'), loaded: true, logOut });
  });

  it('renders nothing when nobody is signed in', () => {
    useAuthStore.setState({ mode: 'open', user: null });
    renderMenu();
    expect(screen.queryByRole('button', { name: /^Account/ })).not.toBeInTheDocument();
  });

  it('is a button named after the signed in account, closed at first', () => {
    renderMenu();
    expect(screen.getByRole('button', { name: 'Account: admin@example.com' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Sign out' })).not.toBeInTheDocument();
  });

  it('shows the email and Admin for an admin', async () => {
    const user = userEvent.setup();
    renderMenu();
    await user.click(screen.getByRole('button', { name: 'Account: admin@example.com' }));

    const menu = screen.getByRole('dialog');
    expect(menu).toHaveTextContent('admin@example.com');
    expect(menu).toHaveTextContent('Admin');
  });

  it('shows Listener for a listener', async () => {
    useAuthStore.setState({ user: account('listener') });
    const user = userEvent.setup();
    renderMenu();
    await user.click(screen.getByRole('button', { name: 'Account: listener@example.com' }));

    const menu = screen.getByRole('dialog');
    expect(menu).toHaveTextContent('listener@example.com');
    expect(menu).toHaveTextContent('Listener');
    expect(menu).not.toHaveTextContent('Admin');
  });

  it('opens account settings and closes itself', async () => {
    const user = userEvent.setup();
    renderMenu();
    await user.click(screen.getByRole('button', { name: 'Account: admin@example.com' }));

    const link = screen.getByRole('link', { name: 'Account settings' });
    expect(link).toHaveAttribute('href', '/account');
    await user.click(link);

    expect(screen.getByText('Account page')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Account settings' })).not.toBeInTheDocument();
  });

  it('signs out', async () => {
    const user = userEvent.setup();
    renderMenu();
    await user.click(screen.getByRole('button', { name: 'Account: admin@example.com' }));
    await user.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(logOut).toHaveBeenCalledTimes(1);
  });

  it('closes with Escape without signing out', async () => {
    const user = userEvent.setup();
    renderMenu();
    await user.click(screen.getByRole('button', { name: 'Account: admin@example.com' }));
    await user.keyboard('{Escape}');

    expect(screen.queryByRole('button', { name: 'Sign out' })).not.toBeInTheDocument();
    expect(logOut).not.toHaveBeenCalled();
  });
});
