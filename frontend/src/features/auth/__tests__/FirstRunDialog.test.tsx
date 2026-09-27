// @vitest-environment jsdom

/**
 * The question every install is asked once: set up accounts, or keep it open. It must not be dismissible
 * without an answer, keeping it open must say so when somebody else answered first, and setting up
 * accounts must go through the new password form into the store's setUp. The auth store's actions are
 * spies, so the requests themselves are the store's and the server's to test.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { ApiError } from '@/api/client';
import { useAuthStore } from '@/store/useAuthStore';

import { FirstRunDialog } from '../FirstRunDialog';

let chooseOpen: Mock<() => Promise<void>>;
let setUp: Mock<(email: string, password: string) => Promise<void>>;
let refresh: Mock<() => Promise<void>>;

const setUpButton = () => screen.getByRole('button', { name: /Set up accounts/ });
const keepOpenButton = () => screen.getByRole('button', { name: /Keep it open/ });

describe('the first run question', () => {
  beforeEach(() => {
    chooseOpen = vi.fn(async () => undefined);
    setUp = vi.fn(async () => undefined);
    refresh = vi.fn(async () => undefined);
    useAuthStore.setState({ mode: 'undecided', user: null, loaded: true, chooseOpen, setUp, refresh });
  });

  it('asks whether to protect the recorder, and explains what open means', () => {
    render(<FirstRunDialog />);
    const dialog = screen.getByRole('dialog', { name: 'Protect this recorder with a login?' });
    expect(dialog).toHaveTextContent('anyone who can reach this page can listen live');
    expect(setUpButton()).toHaveTextContent('You become the admin');
    expect(keepOpenButton()).toHaveTextContent('You can switch accounts on later in Settings.');
  });

  it('offers no close button', () => {
    render(<FirstRunDialog />);
    expect(screen.queryByRole('button', { name: 'Close' })).not.toBeInTheDocument();
  });

  it('stays up when Escape is pressed', async () => {
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.keyboard('{Escape}');
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('keeps the recorder open when asked to', async () => {
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(keepOpenButton());
    expect(chooseOpen).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('disables Keep it open while the answer is being sent', async () => {
    let finish: () => void = () => undefined;
    chooseOpen.mockImplementation(() => new Promise<void>((resolve) => (finish = resolve)));
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(keepOpenButton());

    expect(keepOpenButton()).toBeDisabled();
    await act(async () => {
      finish();
    });
    expect(keepOpenButton()).toBeEnabled();
  });

  it('says why when somebody else answered first, and fetches their answer', async () => {
    chooseOpen.mockRejectedValue(new ApiError('the recorder has already been set up', 'conflict', 409));
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(keepOpenButton());

    expect(await screen.findByRole('alert')).toHaveTextContent('The recorder has already been set up');
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('says something went wrong when the request fails without a server message', async () => {
    chooseOpen.mockRejectedValue(new TypeError('Failed to fetch'));
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(keepOpenButton());
    expect(await screen.findByRole('alert')).toHaveTextContent('Something went wrong. Try again.');
  });

  it('moves to creating the admin account when accounts are chosen', async () => {
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(setUpButton());

    const dialog = screen.getByRole('dialog', { name: 'Create the admin account' });
    expect(dialog).toHaveTextContent('There is no email sent anywhere');
    expect(screen.getByLabelText('Email')).toBeInTheDocument();
    expect(screen.getByLabelText('Password')).toBeInTheDocument();
    expect(screen.getByLabelText('Password again')).toBeInTheDocument();
    expect(chooseOpen).not.toHaveBeenCalled();
  });

  it('creates the admin account with the email and password typed', async () => {
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(setUpButton());
    await user.type(screen.getByLabelText('Email'), 'admin@example.com');
    await user.type(screen.getByLabelText('Password'), 'a long sentence');
    await user.type(screen.getByLabelText('Password again'), 'a long sentence');
    await user.click(screen.getByRole('button', { name: 'Create and sign in' }));

    expect(setUp).toHaveBeenCalledWith('admin@example.com', 'a long sentence');
  });

  it('holds a new password to the minimum length before creating the account', async () => {
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(setUpButton());
    await user.type(screen.getByLabelText('Email'), 'admin@example.com');
    await user.type(screen.getByLabelText('Password'), 'short');
    await user.type(screen.getByLabelText('Password again'), 'short');
    await user.click(screen.getByRole('button', { name: 'Create and sign in' }));

    expect(screen.getByRole('alert')).toHaveTextContent('Use at least 8 characters.');
    expect(setUp).not.toHaveBeenCalled();
  });

  it('shows the server message when creating the account is refused', async () => {
    setUp.mockRejectedValue(new ApiError('accounts are already set up', 'conflict', 409));
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(setUpButton());
    await user.type(screen.getByLabelText('Email'), 'admin@example.com');
    await user.type(screen.getByLabelText('Password'), 'a long sentence');
    await user.type(screen.getByLabelText('Password again'), 'a long sentence');
    await user.click(screen.getByRole('button', { name: 'Create and sign in' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Accounts are already set up');
  });

  it('goes back to the question from the account form', async () => {
    const user = userEvent.setup();
    render(<FirstRunDialog />);
    await user.click(setUpButton());
    await user.click(screen.getByRole('button', { name: 'Back' }));

    expect(screen.getByRole('dialog', { name: 'Protect this recorder with a login?' })).toBeInTheDocument();
    expect(setUp).not.toHaveBeenCalled();
  });
});
