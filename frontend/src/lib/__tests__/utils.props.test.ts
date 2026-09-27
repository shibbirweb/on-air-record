/**
 * Properties of `cn`, the class name merge every component leans on.
 *
 * Components pass it defaults, conditionals and a caller's overrides in one call, so the rules checked
 * here are the ones a component author assumes without thinking: falsy entries vanish, the result is a
 * clean space separated list, merging twice changes nothing, and for two conflicting utilities the later
 * one wins however many other classes sit around them.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { cn } from '../utils';

/** Plain class names Tailwind knows nothing about, so they are kept as given. */
const plainClass = fc.stringMatching(/^[a-z]{3,8}-[a-z]{3,8}$/).map((name) => `x-${name}`);

const entry = fc.oneof(
  plainClass,
  fc.constantFrom('p-1', 'p-2', 'px-4', 'text-sm', 'text-lg', 'bg-red-500', 'bg-blue-500', 'flex', 'hidden'),
  fc.constantFrom(false, null, undefined, ''),
);

describe('cn', () => {
  it('drops falsy entries without trace', () => {
    fc.assert(
      fc.property(fc.array(entry, { maxLength: 10 }), (entries) => {
        const truthy = entries.filter((value) => Boolean(value));
        expect(cn(...entries)).toBe(cn(...truthy));
      }),
    );
  });

  it('returns a clean space separated list', () => {
    fc.assert(
      fc.property(fc.array(entry, { maxLength: 10 }), (entries) => {
        const merged = cn(...entries);
        expect(merged).toBe(merged.trim());
        expect(merged).not.toMatch(/\s{2,}/);
      }),
    );
  });

  it('changes nothing when merged a second time', () => {
    fc.assert(
      fc.property(fc.array(entry, { maxLength: 10 }), (entries) => {
        const merged = cn(...entries);
        expect(cn(merged)).toBe(merged);
      }),
    );
  });

  it('keeps every plain class, and lets the later of two paddings win', () => {
    fc.assert(
      fc.property(
        fc.array(plainClass, { maxLength: 6 }),
        fc.integer({ min: 0, max: 12 }),
        fc.integer({ min: 0, max: 12 }),
        (plain, first, second) => {
          const merged = cn(`p-${first}`, ...plain, `p-${second}`).split(' ');
          expect(merged).toContain(`p-${second}`);
          if (first !== second) {
            expect(merged).not.toContain(`p-${first}`);
          }
          for (const name of plain) {
            expect(merged).toContain(name);
          }
        },
      ),
    );
  });
});
