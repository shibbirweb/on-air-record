/**
 * The credential rules `credentials.test.ts` leaves out: the upper length limit and the exported limits
 * themselves, which must match `PASSWORD_LENGTH` in `backend/src/services/auth_service.rs`, and which of
 * several problems a form is told about first.
 */

import { describe, expect, it } from 'vitest';

import { emailProblem, PASSWORD_MAX_LENGTH, PASSWORD_MIN_LENGTH, passwordProblem } from '../credentials';

describe('password limits', () => {
  it('are eight and one hundred and twenty eight characters, like the server', () => {
    expect(PASSWORD_MIN_LENGTH).toBe(8);
    expect(PASSWORD_MAX_LENGTH).toBe(128);
  });

  it('accepts exactly the longest password and refuses one character more', () => {
    expect(passwordProblem('x'.repeat(PASSWORD_MAX_LENGTH))).toBeNull();
    expect(passwordProblem('x'.repeat(PASSWORD_MAX_LENGTH + 1))).toBe('Use at most 128 characters.');
  });

  it('says how short is too short', () => {
    expect(passwordProblem('x'.repeat(PASSWORD_MIN_LENGTH - 1))).toBe('Use at least 8 characters.');
  });

  it('counts emoji by code point at the upper limit too', () => {
    expect(passwordProblem('\u{1F399}'.repeat(PASSWORD_MAX_LENGTH))).toBeNull();
    expect(passwordProblem('\u{1F399}'.repeat(PASSWORD_MAX_LENGTH + 1))).not.toBeNull();
  });

  it('reports the length before a confirmation that does not match', () => {
    expect(passwordProblem('short', 'different')).toBe('Use at least 8 characters.');
  });

  it('treats an empty confirmation as given, and so as not matching', () => {
    expect(passwordProblem('a long password', '')).toBe('The two passwords do not match.');
  });
});

describe('emailProblem wording', () => {
  it('asks for an email address in one plain sentence', () => {
    expect(emailProblem('nobody')).toBe('Enter an email address.');
  });

  it('refuses whitespace inside the address, tabs included', () => {
    expect(emailProblem('own\ter@example.com')).not.toBeNull();
  });
});
