/**
 * Properties of the client side email and password checks, over generated strings.
 *
 * These mirror rules the server enforces, so the useful failures are disagreements: a password the form
 * rejects that the server would take, or the reverse, most likely with characters outside the Basic
 * Multilingual Plane, where UTF-16 length and code point count part ways. Each rule is restated here
 * independently and compared with the implementation over arbitrary text.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { capitalise, emailProblem, PASSWORD_MAX_LENGTH, PASSWORD_MIN_LENGTH, passwordProblem } from '../credentials';

/** Whitespace as JavaScript's `trim` and `\s` both define it. */
const whitespace = fc.constantFrom(' ', '\t', '\n', '\r', '\u00A0', '\u2028', '\u3000', '\uFEFF');

describe('emailProblem', () => {
  it('accepts exactly a name, an @ and something after it, with no whitespace inside', () => {
    fc.assert(
      fc.property(
        fc.oneof(fc.string({ unit: 'binary', maxLength: 30 }), fc.stringMatching(/^[a-z@. \t]{0,12}$/)),
        (email) => {
          const trimmed = email.trim();
          const at = trimmed.indexOf('@');
          const valid = at > 0 && at < trimmed.length - 1 && !/\s/.test(trimmed);
          expect(emailProblem(email) === null).toBe(valid);
        },
      ),
    );
  });

  it('ignores whitespace around the address', () => {
    fc.assert(
      fc.property(
        fc.string({ unit: 'binary', maxLength: 30 }),
        fc.array(whitespace),
        fc.array(whitespace),
        (email, before, after) => {
          expect(emailProblem(`${before.join('')}${email}${after.join('')}`)).toBe(emailProblem(email));
        },
      ),
    );
  });

  it('accepts any name and domain free of whitespace, however unusual', () => {
    const part = fc.string({ unit: 'binary', minLength: 1, maxLength: 20 }).filter((text) => !/[\s@]/.test(text));
    fc.assert(
      fc.property(part, part, (name, domain) => {
        expect(emailProblem(`${name}@${domain}`)).toBeNull();
      }),
    );
  });
});

describe('passwordProblem', () => {
  /** Code points, whole characters outside the Basic Multilingual Plane among them. */
  const password = fc.string({ unit: 'binary', maxLength: PASSWORD_MAX_LENGTH + 20 });

  it('accepts exactly the lengths the server does, counted in code points', () => {
    fc.assert(
      fc.property(password, (value) => {
        const length = [...value].length;
        const valid = length >= PASSWORD_MIN_LENGTH && length <= PASSWORD_MAX_LENGTH;
        expect(passwordProblem(value) === null).toBe(valid);
      }),
    );
  });

  it('counts an emoji as one character, not two', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: PASSWORD_MAX_LENGTH }),
        fc.constantFrom('\u{1F3A4}', '\u{1D11E}', '\u{20000}'),
        (count, astral) => {
          const value = astral.repeat(count);
          expect(passwordProblem(value) === null).toBe(count >= PASSWORD_MIN_LENGTH);
        },
      ),
    );
  });

  it('asks for a matching confirmation only once the length is right', () => {
    fc.assert(
      fc.property(password, password, (value, confirmation) => {
        const lengthProblem = passwordProblem(value);
        const withConfirmation = passwordProblem(value, confirmation);
        if (lengthProblem !== null) {
          expect(withConfirmation).toBe(lengthProblem);
        } else {
          expect(withConfirmation === null).toBe(confirmation === value);
        }
        expect(passwordProblem(value, value)).toBe(lengthProblem);
      }),
    );
  });
});

describe('capitalise', () => {
  it('leaves everything after the first character untouched', () => {
    fc.assert(
      fc.property(fc.string({ unit: 'binary', minLength: 1, maxLength: 40 }), (message) => {
        const rest = message.slice(1);
        expect(capitalise(message).endsWith(rest)).toBe(true);
        expect(capitalise(message).startsWith(message.charAt(0).toUpperCase())).toBe(true);
      }),
    );
  });

  it('upper cases a lower case ASCII start and is idempotent on ASCII', () => {
    fc.assert(
      fc.property(fc.stringMatching(/^[a-z][ -~]{0,40}$/), (message) => {
        const once = capitalise(message);
        expect(once[0]).toBe(message[0].toUpperCase());
        expect(capitalise(once)).toBe(once);
      }),
    );
  });
});
