// @vitest-environment jsdom

/**
 * The email and password form shared by first run setup, the login page and adding an account. It checks
 * the same rules as the server before sending, so these tests pin what each mistake says and that nothing
 * is sent while one stands; the submit handler is a spy, so what it does with the answer is its caller's
 * business and tested there.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { ApiError } from '@/api/client';

import { CredentialsForm } from '../CredentialsForm';

let onSubmit: Mock<(email: string, password: string) => Promise<void>>;

const emailField = () => screen.getByLabelText('Email');
const passwordField = () => screen.getByLabelText('Password');
const confirmationField = () => screen.getByLabelText('Password again');

describe('the credentials form for signing in', () => {
  beforeEach(() => {
    onSubmit = vi.fn(async () => undefined);
  });

  it('asks for an email and a password, with the button named by its caller', () => {
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    expect(emailField()).toHaveAttribute('type', 'email');
    expect(passwordField()).toHaveAttribute('type', 'password');
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled();
    expect(screen.queryByLabelText('Password again')).not.toBeInTheDocument();
  });

  it('puts the cursor in the email field', () => {
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    expect(emailField()).toHaveFocus();
  });

  it('lets the password manager fill a saved login', () => {
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    expect(emailField()).toHaveAttribute('autocomplete', 'username');
    expect(passwordField()).toHaveAttribute('autocomplete', 'current-password');
  });

  it('sends the email without surrounding spaces and the password exactly as typed', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(emailField(), '  admin@example.com ');
    await user.type(passwordField(), ' pass word ');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));

    expect(onSubmit).toHaveBeenCalledWith('admin@example.com', ' pass word ');
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('submits when Enter is pressed in the password field', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'secret{Enter}');
    expect(onSubmit).toHaveBeenCalledWith('admin@example.com', 'secret');
  });

  it('asks for an email when the field is empty, and sends nothing', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(passwordField(), 'secret');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));

    expect(screen.getByRole('alert')).toHaveTextContent('Enter an email address.');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('asks for an email when what was typed has no @', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin');
    await user.type(passwordField(), 'secret');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));

    expect(screen.getByRole('alert')).toHaveTextContent('Enter an email address.');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('asks for the password when it is empty, and sends nothing', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));

    expect(screen.getByRole('alert')).toHaveTextContent('Enter your password.');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('accepts a short password, since only a new password has a minimum length', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'abc');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(onSubmit).toHaveBeenCalledWith('admin@example.com', 'abc');
  });

  it('shows the server message, starting with a capital, when the request is refused', async () => {
    onSubmit.mockRejectedValue(new ApiError('the email or password is not right', 'unauthorized', 401));
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'wrong');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('The email or password is not right');
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled();
  });

  it('says something went wrong when the failure is not the server speaking', async () => {
    onSubmit.mockRejectedValue(new TypeError('Failed to fetch'));
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'secret');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Something went wrong. Try again.');
  });

  it('disables the button while the request is on its way, and enables it again after', async () => {
    let finish: () => void = () => undefined;
    onSubmit.mockImplementation(() => new Promise<void>((resolve) => (finish = resolve)));
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'secret');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));

    expect(screen.getByRole('button', { name: 'Sign in' })).toBeDisabled();
    await act(async () => {
      finish();
    });
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeEnabled();
  });

  it('clears an old message once a corrected attempt goes out', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Sign in" onSubmit={onSubmit} />);
    await user.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(screen.getByRole('alert')).toBeInTheDocument();

    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'secret');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('shows extra fields from its caller above the button', () => {
    render(
      <CredentialsForm submitLabel="Add account" onSubmit={onSubmit}>
        <p>Role picker</p>
      </CredentialsForm>,
    );
    const extra = screen.getByText('Role picker');
    const button = screen.getByRole('button', { name: 'Add account' });
    expect(extra.compareDocumentPosition(button) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe('the credentials form for a new password', () => {
  beforeEach(() => {
    onSubmit = vi.fn(async () => undefined);
  });

  it('asks for the password twice and says how long it must be', () => {
    render(<CredentialsForm submitLabel="Create and sign in" newPassword onSubmit={onSubmit} />);
    expect(confirmationField()).toHaveAttribute('type', 'password');
    expect(screen.getByText('At least 8 characters. A short sentence works well.')).toBeInTheDocument();
  });

  it('asks the password manager for a new password rather than a saved one', () => {
    render(<CredentialsForm submitLabel="Create and sign in" newPassword onSubmit={onSubmit} />);
    expect(emailField()).toHaveAttribute('autocomplete', 'email');
    expect(passwordField()).toHaveAttribute('autocomplete', 'new-password');
    expect(confirmationField()).toHaveAttribute('autocomplete', 'new-password');
  });

  it('refuses a password shorter than 8 characters', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Create and sign in" newPassword onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'short');
    await user.type(confirmationField(), 'short');
    await user.click(screen.getByRole('button', { name: 'Create and sign in' }));

    expect(screen.getByRole('alert')).toHaveTextContent('Use at least 8 characters.');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('refuses a password longer than 128 characters', async () => {
    const user = userEvent.setup();
    const long = 'a'.repeat(129);
    render(<CredentialsForm submitLabel="Create and sign in" newPassword onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.click(passwordField());
    await user.paste(long);
    await user.click(confirmationField());
    await user.paste(long);
    await user.click(screen.getByRole('button', { name: 'Create and sign in' }));

    expect(screen.getByRole('alert')).toHaveTextContent('Use at most 128 characters.');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('refuses two passwords that differ', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Create and sign in" newPassword onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'a long sentence');
    await user.type(confirmationField(), 'a long sentense');
    await user.click(screen.getByRole('button', { name: 'Create and sign in' }));

    expect(screen.getByRole('alert')).toHaveTextContent('The two passwords do not match.');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('checks the email before the password', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Create and sign in" newPassword onSubmit={onSubmit} />);
    await user.type(passwordField(), 'short');
    await user.click(screen.getByRole('button', { name: 'Create and sign in' }));
    expect(screen.getByRole('alert')).toHaveTextContent('Enter an email address.');
  });

  it('sends the email and the password once both passwords match', async () => {
    const user = userEvent.setup();
    render(<CredentialsForm submitLabel="Create and sign in" newPassword onSubmit={onSubmit} />);
    await user.type(emailField(), 'admin@example.com');
    await user.type(passwordField(), 'a long sentence');
    await user.type(confirmationField(), 'a long sentence');
    await user.click(screen.getByRole('button', { name: 'Create and sign in' }));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith('admin@example.com', 'a long sentence');
  });
});
