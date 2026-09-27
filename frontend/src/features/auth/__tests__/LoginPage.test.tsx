// @vitest-environment jsdom

/**
 * The page shown instead of the app when accounts are on and nobody is signed in: the password step, and
 * for an account with two factor sign in, the code step after it. The auth store's actions are spies, so
 * these test which action each step calls with what, what each failure says, and that a right password on
 * a two factor account moves the page to the code rather than into the app.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { ApiError } from '@/api/client';
import { useAuthStore } from '@/store/useAuthStore';

import { LoginPage } from '../LoginPage';

let logIn: Mock<(email: string, password: string) => Promise<void>>;
let verifyCode: Mock<(code: string) => Promise<void>>;
let backToPassword: Mock<() => Promise<void>>;
let refresh: Mock<() => Promise<void>>;

const codeField = () => screen.getByLabelText('Code');
const signInButton = () => screen.getByRole('button', { name: 'Sign in' });

describe('the password step', () => {
  beforeEach(() => {
    logIn = vi.fn(async () => undefined);
    verifyCode = vi.fn(async () => undefined);
    backToPassword = vi.fn(async () => undefined);
    refresh = vi.fn(async () => undefined);
    useAuthStore.setState({
      mode: 'accounts',
      user: null,
      pendingTwoFactor: false,
      loaded: true,
      logIn,
      verifyCode,
      backToPassword,
      refresh,
    });
  });

  it('names the app and asks to sign in', () => {
    render(<LoginPage />);
    expect(screen.getByRole('heading', { level: 1, name: 'On Air Record' })).toBeInTheDocument();
    expect(screen.getByText('Sign in', { selector: '[data-slot="card-title"]' })).toBeInTheDocument();
    expect(screen.getByText('This recorder needs an account to listen or make changes.')).toBeInTheDocument();
    expect(screen.getByLabelText('Email')).toBeInTheDocument();
    expect(screen.getByLabelText('Password')).toBeInTheDocument();
    expect(screen.queryByLabelText('Password again')).not.toBeInTheDocument();
  });

  it('says what to do about a forgotten password, including the command on the host', () => {
    render(<LoginPage />);
    expect(screen.getByText(/Forgot your password\? Ask an admin to set a new one/)).toBeInTheDocument();
    expect(screen.getByText('on-air-record auth reset-password')).toBeInTheDocument();
  });

  it('signs in with the email and password typed', async () => {
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(screen.getByLabelText('Email'), 'listener@example.com');
    await user.type(screen.getByLabelText('Password'), 'secret');
    await user.click(signInButton());
    expect(logIn).toHaveBeenCalledWith('listener@example.com', 'secret');
  });

  it('shows why a sign in was refused', async () => {
    logIn.mockRejectedValue(new ApiError('the email or password is not right', 'unauthorized', 401));
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(screen.getByLabelText('Email'), 'listener@example.com');
    await user.type(screen.getByLabelText('Password'), 'wrong');
    await user.click(signInButton());
    expect(await screen.findByRole('alert')).toHaveTextContent('The email or password is not right');
  });

  it('moves to the code step when the password was right on a two factor account', async () => {
    logIn.mockImplementation(async () => {
      useAuthStore.setState({ pendingTwoFactor: true });
    });
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(screen.getByLabelText('Email'), 'admin@example.com');
    await user.type(screen.getByLabelText('Password'), 'secret');
    await user.click(signInButton());

    expect(await screen.findByText('Enter your code')).toBeInTheDocument();
    expect(screen.queryByLabelText('Password')).not.toBeInTheDocument();
  });
});

describe('the code step', () => {
  beforeEach(() => {
    logIn = vi.fn(async () => undefined);
    verifyCode = vi.fn(async () => undefined);
    backToPassword = vi.fn(async () => undefined);
    refresh = vi.fn(async () => undefined);
    useAuthStore.setState({
      mode: 'accounts',
      user: null,
      pendingTwoFactor: true,
      loaded: true,
      logIn,
      verifyCode,
      backToPassword,
      refresh,
    });
  });

  it('asks for the code from the authenticator app, with the cursor already in the field', () => {
    render(<LoginPage />);
    expect(screen.getByText('Enter your code')).toBeInTheDocument();
    expect(screen.getByText(/type the 6 digit code it shows for On Air Record/)).toBeInTheDocument();
    expect(codeField()).toHaveFocus();
    expect(codeField()).toHaveAttribute('autocomplete', 'one-time-code');
    expect(screen.queryByLabelText('Email')).not.toBeInTheDocument();
  });

  it('says a recovery code works in place of the phone', () => {
    render(<LoginPage />);
    expect(screen.getByText('No phone to hand? Type one of your recovery codes instead.')).toBeInTheDocument();
  });

  it('asks for the code when the field is empty, and sends nothing', async () => {
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(codeField(), '   ');
    await user.click(signInButton());

    expect(screen.getByRole('alert')).toHaveTextContent('Enter the code from your authenticator app.');
    expect(verifyCode).not.toHaveBeenCalled();
  });

  it('sends the code without surrounding spaces', async () => {
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(codeField(), ' 123456 ');
    await user.click(signInButton());
    expect(verifyCode).toHaveBeenCalledWith('123456');
  });

  it('sends a recovery code the same way', async () => {
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(codeField(), 'abcd-efgh{Enter}');
    expect(verifyCode).toHaveBeenCalledWith('abcd-efgh');
  });

  it('disables Sign in while the code is being checked', async () => {
    let finish: () => void = () => undefined;
    verifyCode.mockImplementation(() => new Promise<void>((resolve) => (finish = resolve)));
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(codeField(), '123456');
    await user.click(signInButton());

    expect(signInButton()).toBeDisabled();
    await act(async () => {
      finish();
    });
    expect(signInButton()).toBeEnabled();
  });

  it('empties the field and says why when the code is wrong, without leaving the step', async () => {
    verifyCode.mockRejectedValue(new ApiError('that code is not right', 'unauthorized', 401));
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(codeField(), '000000');
    await user.click(signInButton());

    expect(await screen.findByRole('alert')).toHaveTextContent('That code is not right');
    expect(codeField()).toHaveValue('');
    expect(refresh).not.toHaveBeenCalled();
  });

  it('asks the server where things stand when the sign in has expired and needs the password again', async () => {
    verifyCode.mockRejectedValue(
      new ApiError('that sign in has expired; enter your password again', 'unauthorized', 401),
    );
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(codeField(), '123456');
    await user.click(signInButton());

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'That sign in has expired; enter your password again',
    );
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('says something went wrong when the failure is not the server speaking', async () => {
    verifyCode.mockRejectedValue(new TypeError('Failed to fetch'));
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.type(codeField(), '123456');
    await user.click(signInButton());
    expect(await screen.findByRole('alert')).toHaveTextContent('Something went wrong. Try again.');
  });

  it('goes back to the password with Use a different account', async () => {
    const user = userEvent.setup();
    render(<LoginPage />);
    await user.click(screen.getByRole('button', { name: 'Use a different account' }));
    expect(backToPassword).toHaveBeenCalledTimes(1);
    expect(verifyCode).not.toHaveBeenCalled();
  });

  it('shows the password step again once the store leaves the code step', async () => {
    render(<LoginPage />);
    act(() => {
      useAuthStore.setState({ pendingTwoFactor: false });
    });
    expect(screen.getByLabelText('Password')).toBeInTheDocument();
    expect(screen.queryByText('Enter your code')).not.toBeInTheDocument();
  });
});
