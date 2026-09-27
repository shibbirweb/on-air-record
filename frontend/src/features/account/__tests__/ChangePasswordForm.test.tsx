// @vitest-environment jsdom

/**
 * Changing your own password on the account settings page: the current password, the new one twice, the
 * same length rules as the server, and on success empty fields and a note that other browsers were signed
 * out. The auth store's changePassword is a spy, so the request is the store's and the server's to test.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { ApiError } from '@/api/client';
import { useAuthStore } from '@/store/useAuthStore';

import { ChangePasswordForm } from '../ChangePasswordForm';

let changePassword: Mock<(currentPassword: string, newPassword: string) => Promise<void>>;

const currentField = () => screen.getByLabelText('Current password');
const nextField = () => screen.getByLabelText('New password');
const confirmationField = () => screen.getByLabelText('New password again');
const submitButton = () => screen.getByRole('button', { name: 'Change password' });

async function fill(
  user: ReturnType<typeof userEvent.setup>,
  current: string,
  next: string,
  confirmation: string = next,
) {
  if (current !== '') {
    await user.type(currentField(), current);
  }
  if (next !== '') {
    await user.type(nextField(), next);
  }
  if (confirmation !== '') {
    await user.type(confirmationField(), confirmation);
  }
}

describe('the change password form', () => {
  beforeEach(() => {
    changePassword = vi.fn(async () => undefined);
    useAuthStore.setState({ changePassword });
  });

  it('asks for the current password and the new one twice, all hidden as typed', () => {
    render(<ChangePasswordForm />);
    expect(currentField()).toHaveAttribute('type', 'password');
    expect(nextField()).toHaveAttribute('type', 'password');
    expect(confirmationField()).toHaveAttribute('type', 'password');
    expect(screen.getByText('At least 8 characters. A short sentence works well.')).toBeInTheDocument();
    expect(submitButton()).toBeEnabled();
  });

  it('lets the password manager fill the current password and offer a new one', () => {
    render(<ChangePasswordForm />);
    expect(currentField()).toHaveAttribute('autocomplete', 'current-password');
    expect(nextField()).toHaveAttribute('autocomplete', 'new-password');
    expect(confirmationField()).toHaveAttribute('autocomplete', 'new-password');
  });

  it('asks for the current password when it is empty, and sends nothing', async () => {
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, '', 'a new sentence');
    await user.click(submitButton());

    expect(screen.getByRole('alert')).toHaveTextContent('Enter your current password.');
    expect(changePassword).not.toHaveBeenCalled();
  });

  it('refuses a new password shorter than 8 characters', async () => {
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, 'old password', 'short');
    await user.click(submitButton());

    expect(screen.getByRole('alert')).toHaveTextContent('Use at least 8 characters.');
    expect(changePassword).not.toHaveBeenCalled();
  });

  it('refuses two new passwords that differ', async () => {
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, 'old password', 'a new sentence', 'a new sentense');
    await user.click(submitButton());

    expect(screen.getByRole('alert')).toHaveTextContent('The two passwords do not match.');
    expect(changePassword).not.toHaveBeenCalled();
  });

  it('changes the password with the current and the new one', async () => {
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, 'old password', 'a new sentence');
    await user.click(submitButton());
    expect(changePassword).toHaveBeenCalledWith('old password', 'a new sentence');
  });

  it('empties every field on success and says other browsers were signed out', async () => {
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, 'old password', 'a new sentence');
    await user.click(submitButton());

    expect(await screen.findByRole('status')).toHaveTextContent(
      'Your password has been changed. Other browsers have been signed out.',
    );
    expect(currentField()).toHaveValue('');
    expect(nextField()).toHaveValue('');
    expect(confirmationField()).toHaveValue('');
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('disables the button while the change is on its way', async () => {
    let finish: () => void = () => undefined;
    changePassword.mockImplementation(() => new Promise<void>((resolve) => (finish = resolve)));
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, 'old password', 'a new sentence');
    await user.click(submitButton());

    expect(submitButton()).toBeDisabled();
    await act(async () => {
      finish();
    });
    expect(submitButton()).toBeEnabled();
  });

  it('shows the server message when the current password is wrong, and says nothing changed', async () => {
    changePassword.mockRejectedValue(new ApiError('the current password is not right', 'unauthorized', 401));
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, 'wrong password', 'a new sentence');
    await user.click(submitButton());

    expect(await screen.findByRole('alert')).toHaveTextContent('The current password is not right');
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(submitButton()).toBeEnabled();
  });

  it('says something went wrong when the failure is not the server speaking', async () => {
    changePassword.mockRejectedValue(new TypeError('Failed to fetch'));
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, 'old password', 'a new sentence');
    await user.click(submitButton());
    expect(await screen.findByRole('alert')).toHaveTextContent('Something went wrong. Try again.');
  });

  it('clears an old message once a corrected attempt goes out', async () => {
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await user.click(submitButton());
    expect(screen.getByRole('alert')).toBeInTheDocument();

    await fill(user, 'old password', 'a new sentence');
    await user.click(submitButton());
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('drops the last success message when a new attempt fails validation', async () => {
    const user = userEvent.setup();
    render(<ChangePasswordForm />);
    await fill(user, 'old password', 'a new sentence');
    await user.click(submitButton());
    expect(await screen.findByText(/Your password has been changed/)).toBeInTheDocument();

    await fill(user, 'another', 'short');
    await user.click(submitButton());
    expect(screen.getByRole('alert')).toHaveTextContent('Use at least 8 characters.');
    expect(screen.queryByText(/Your password has been changed/)).not.toBeInTheDocument();
  });
});
