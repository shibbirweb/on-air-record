// @vitest-environment jsdom

/**
 * The two factor card on the account settings page: whether it is on, how many recovery codes are left,
 * and the button into the dialog. The status is read on arrival and again when the dialog closes, since
 * the dialog may have changed it. The two factor store's actions are spies; the dialog's own steps are
 * tested in TwoFactorDialog.test.tsx.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { TwoFactorSetup } from '@/api/types';
import { useTwoFactorStore } from '@/store/useTwoFactorStore';

import { TwoFactorSettings } from '../TwoFactorSettings';

let refresh: Mock<() => Promise<void>>;

describe('the two factor card', () => {
  beforeEach(() => {
    refresh = vi.fn(async () => undefined);
    useTwoFactorStore.setState({
      status: { enabled: false, recoveryCodesLeft: 0 },
      refresh,
      beginSetup: vi.fn(async (): Promise<TwoFactorSetup> => ({ secretKey: '', otpauthUri: '', qrSvg: '' })),
      enable: vi.fn(async () => []),
      disable: vi.fn(async () => undefined),
      regenerate: vi.fn(async () => []),
    });
  });

  it('reads the status when it appears', () => {
    render(<TwoFactorSettings />);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('says it is checking until the status is known', () => {
    useTwoFactorStore.setState({ status: null });
    render(<TwoFactorSettings />);
    expect(screen.getByText('Checking...')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Set up' })).toBeInTheDocument();
  });

  it('says it is off, what that means, and offers Set up', () => {
    render(<TwoFactorSettings />);
    expect(screen.getByText('Off')).toBeInTheDocument();
    expect(
      screen.getByText('Signing in needs only your password. Add a code from an authenticator app on your phone.'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Set up' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Manage' })).not.toBeInTheDocument();
  });

  it('says it is on with the recovery codes left, and offers Manage', () => {
    useTwoFactorStore.setState({ status: { enabled: true, recoveryCodesLeft: 4 } });
    render(<TwoFactorSettings />);
    expect(screen.getByText('On')).toBeInTheDocument();
    expect(
      screen.getByText('Signing in asks for a code from your authenticator app. 4 of 10 recovery codes left.'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Manage' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Set up' })).not.toBeInTheDocument();
  });

  it('opens the two factor dialog', async () => {
    const user = userEvent.setup();
    render(<TwoFactorSettings />);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Set up' }));
    expect(screen.getByRole('dialog', { name: 'Two factor sign in' })).toBeInTheDocument();
  });

  it('reads the status again when the dialog closes', async () => {
    const user = userEvent.setup();
    render(<TwoFactorSettings />);
    await user.click(screen.getByRole('button', { name: 'Set up' }));
    refresh.mockClear();

    await user.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('shows the new status once the store has it', async () => {
    const user = userEvent.setup();
    refresh.mockImplementation(async () => {
      useTwoFactorStore.setState({ status: { enabled: true, recoveryCodesLeft: 10 } });
    });
    useTwoFactorStore.setState({ status: null });
    render(<TwoFactorSettings />);

    expect(await screen.findByText('On')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Manage' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Manage' }));
    expect(screen.getByRole('dialog')).toHaveTextContent('On. 10 of 10 recovery codes left.');
  });
});
