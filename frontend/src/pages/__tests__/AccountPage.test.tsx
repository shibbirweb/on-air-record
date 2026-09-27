// @vitest-environment jsdom

/**
 * The account settings page: open to every signed in account, listeners included, and sent back to the
 * control room when nobody is signed in, since without accounts there is no account to set. The stores'
 * actions are spies so rendering the cards sends nothing anywhere.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { User } from '@/api/types';
import { useAuthStore } from '@/store/useAuthStore';
import { useTwoFactorStore } from '@/store/useTwoFactorStore';

import { AccountPage } from '../AccountPage';

function account(role: User['role']): User {
  return { id: 1, email: `${role}@example.com`, role, createdAtMs: 0, twoFactorEnabled: false };
}

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/" element={<p>Control room page</p>} />
        <Route path="/account" element={<AccountPage />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe('the account settings page', () => {
  beforeEach(() => {
    useAuthStore.setState({ mode: 'accounts', user: account('admin'), changePassword: vi.fn(async () => undefined) });
    useTwoFactorStore.setState({
      status: { enabled: false, recoveryCodesLeft: 0 },
      refresh: vi.fn(async () => undefined),
    });
  });

  it('sends a visitor back to the control room when nobody is signed in', () => {
    useAuthStore.setState({ mode: 'open', user: null });
    renderAt('/account');
    expect(screen.getByText('Control room page')).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Account settings' })).not.toBeInTheDocument();
  });

  it('names the signed in account and says it is an admin', () => {
    renderAt('/account');
    expect(screen.getByRole('heading', { name: 'Account settings' })).toBeInTheDocument();
    expect(screen.getByText('admin@example.com')).toHaveTextContent('Admin');
  });

  it('is open to a listener too, and says so', () => {
    useAuthStore.setState({ user: account('listener') });
    renderAt('/account');
    expect(screen.getByRole('heading', { name: 'Account settings' })).toBeInTheDocument();
    expect(screen.getByText('listener@example.com')).toHaveTextContent('Listener');
  });

  it('has a password card that says other browsers are signed out when it changes', () => {
    renderAt('/account');
    expect(screen.getByText('Password', { selector: '[data-slot="card-title"]' })).toBeInTheDocument();
    expect(screen.getByText(/Other browsers signed in to this account are signed out/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Change password' })).toBeInTheDocument();
  });

  it('has a two factor card with the way to set it up', () => {
    renderAt('/account');
    expect(screen.getByText('Two factor sign in', { selector: '[data-slot="card-title"]' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Set up' })).toBeInTheDocument();
  });
});
