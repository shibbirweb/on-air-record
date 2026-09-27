// @vitest-environment jsdom

/**
 * The gate above the app shell: it decides between waiting, an unreachable recorder, the login page, the
 * app, and the app with the first run question over it. With accounts on and nobody signed in the app must
 * not render at all, because the shell builds the audio engine and opens the stream. The auth store's
 * refresh is a spy, and the client's unauthorized handler is watched, so a 401 anywhere is proven to send
 * the page back through here.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { User } from '@/api/types';

const client = vi.hoisted(() => ({
  handler: null as (() => void) | null,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  return {
    ...actual,
    setUnauthorizedHandler: (handler: (() => void) | null) => {
      client.handler = handler;
    },
  };
});

const { useAuthStore } = await import('@/store/useAuthStore');
const { AuthGate } = await import('../AuthGate');

const ADMIN: User = {
  id: 1,
  email: 'admin@example.com',
  role: 'admin',
  createdAtMs: 0,
  twoFactorEnabled: false,
};

let refresh: Mock<() => Promise<void>>;

function renderGate() {
  return render(
    <AuthGate>
      <p>The app</p>
    </AuthGate>,
  );
}

describe('the auth gate', () => {
  beforeEach(() => {
    client.handler = null;
    refresh = vi.fn(async () => undefined);
    useAuthStore.setState({
      mode: null,
      user: null,
      pendingTwoFactor: false,
      loaded: false,
      error: null,
      refresh,
    });
  });

  it('asks the server who is signed in when it first appears', () => {
    renderGate();
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('shows only a loading sign until the first answer arrives', () => {
    renderGate();
    expect(screen.getByLabelText('Loading')).toBeInTheDocument();
    expect(screen.queryByText('The app')).not.toBeInTheDocument();
    expect(screen.queryByText('Sign in', { selector: '[data-slot="card-title"]' })).not.toBeInTheDocument();
  });

  it('says the recorder cannot be reached, and tries again on request', async () => {
    useAuthStore.setState({ loaded: true, error: 'the service is unreachable' });
    const user = userEvent.setup();
    renderGate();

    expect(screen.getByText('The recorder cannot be reached: the service is unreachable')).toBeInTheDocument();
    expect(screen.queryByText('The app')).not.toBeInTheDocument();
    refresh.mockClear();
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('shows the login page and not the app when accounts are on and nobody is signed in', () => {
    useAuthStore.setState({ loaded: true, mode: 'accounts' });
    renderGate();
    expect(screen.getByText('Sign in', { selector: '[data-slot="card-title"]' })).toBeInTheDocument();
    expect(screen.queryByText('The app')).not.toBeInTheDocument();
  });

  it('shows the app to somebody signed in', () => {
    useAuthStore.setState({ loaded: true, mode: 'accounts', user: ADMIN });
    renderGate();
    expect(screen.getByText('The app')).toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows the app to anyone when the recorder is open', () => {
    useAuthStore.setState({ loaded: true, mode: 'open' });
    renderGate();
    expect(screen.getByText('The app')).toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows the app with the first run question over it before anyone has chosen', () => {
    useAuthStore.setState({ loaded: true, mode: 'undecided' });
    renderGate();
    expect(screen.getByText('The app')).toBeInTheDocument();
    expect(screen.getByRole('dialog', { name: 'Protect this recorder with a login?' })).toBeInTheDocument();
  });

  it('moves from the login page to the app once somebody signs in', () => {
    useAuthStore.setState({ loaded: true, mode: 'accounts' });
    renderGate();
    act(() => {
      useAuthStore.setState({ user: ADMIN });
    });
    expect(screen.getByText('The app')).toBeInTheDocument();
  });

  it('asks the server again whenever a request finds the session gone', () => {
    renderGate();
    expect(client.handler).not.toBeNull();
    refresh.mockClear();
    act(() => {
      client.handler?.();
    });
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('stops listening for ended sessions when it goes away', () => {
    const { unmount } = renderGate();
    unmount();
    expect(client.handler).toBeNull();
  });
});
