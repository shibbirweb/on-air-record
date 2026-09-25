import { describe, expect, it } from 'vitest';

import { canAdminister } from '@/store/useAuthStore';
import type { User } from '@/api/types';
import { capitalise, emailProblem, passwordProblem } from '../credentials';

describe('emailProblem', () => {
  it('accepts an ordinary address, spaces around it included', () => {
    expect(emailProblem('  owner@example.com ')).toBeNull();
  });

  it('refuses what cannot be a login name', () => {
    for (const bad of ['', 'owner', '@example.com', 'owner@', 'own er@example.com']) {
      expect(emailProblem(bad)).not.toBeNull();
    }
  });
});

describe('passwordProblem', () => {
  it('needs at least eight characters', () => {
    expect(passwordProblem('short')).not.toBeNull();
    expect(passwordProblem('eight ch')).toBeNull();
  });

  it('counts an emoji as one character, like the server', () => {
    expect(passwordProblem('\u{1F399}'.repeat(8))).toBeNull();
    expect(passwordProblem('\u{1F399}'.repeat(7))).not.toBeNull();
  });

  it('checks the confirmation only when one is given', () => {
    expect(passwordProblem('a long password', 'a long password')).toBeNull();
    expect(passwordProblem('a long password', 'a long passwore')).toMatch(/do not match/);
  });
});

describe('canAdminister', () => {
  const user = (role: User['role']): User => ({
    id: 1,
    email: 'a@b.c',
    role,
    createdAtMs: 0,
    twoFactorEnabled: false,
  });

  it('lets anyone change things without accounts', () => {
    expect(canAdminister('open', null)).toBe(true);
    expect(canAdminister('undecided', null)).toBe(true);
  });

  it('keeps listeners to listening once accounts are on', () => {
    expect(canAdminister('accounts', user('admin'))).toBe(true);
    expect(canAdminister('accounts', user('listener'))).toBe(false);
    expect(canAdminister('accounts', null)).toBe(false);
  });
});

describe('capitalise', () => {
  it('starts a server message with a capital for display on its own', () => {
    expect(capitalise('the email or password is not right')).toBe('The email or password is not right');
    expect(capitalise('')).toBe('');
  });
});
