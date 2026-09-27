/**
 * Properties of the silent keep-alive clip and the check that decides which browsers play it.
 *
 * The WAV is built by hand, so its header has to agree with its length for every duration and rate, not
 * only the default ten seconds at 8 kHz: a mismatched size field is exactly what makes one browser play a
 * file and another reject it. The browser check has to fall on the right side for any user agent built
 * from the fragments that decide it, in any order and with anything around them.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { needsNotificationKeeper, silentWav } from '../silence';

function ascii(bytes: Uint8Array, offset: number, length: number): string {
  return String.fromCharCode(...bytes.subarray(offset, offset + length));
}

describe('silentWav', () => {
  it('writes a header that agrees with the file for any duration and rate', () => {
    fc.assert(
      fc.property(
        fc.double({ min: 0, max: 20, noNaN: true }),
        fc.integer({ min: 1, max: 48_000 }),
        (seconds, sampleRate) => {
          const bytes = silentWav(seconds, sampleRate);
          const view = new DataView(bytes.buffer);
          const samples = Math.max(Math.round(seconds * sampleRate), 1);

          expect(bytes.length).toBe(44 + samples);
          expect(ascii(bytes, 0, 4)).toBe('RIFF');
          expect(view.getUint32(4, true)).toBe(bytes.length - 8);
          expect(ascii(bytes, 8, 8)).toBe('WAVEfmt ');
          expect(view.getUint32(24, true)).toBe(sampleRate);
          expect(view.getUint32(28, true)).toBe(sampleRate);
          expect(ascii(bytes, 36, 4)).toBe('data');
          expect(view.getUint32(40, true)).toBe(bytes.length - 44);
          // Eight bit WAV is unsigned, so every sample of silence is the midpoint.
          expect(bytes.subarray(44).every((value) => value === 128)).toBe(true);
        },
      ),
      { numRuns: 50 },
    );
  });
});

/** Text that carries none of the engine names the check looks for. */
const filler = fc.stringMatching(/^[a-z0-9 ;()./_-]{0,20}$/);

const engineToken = /iPhone|iPad|iPod|Safari|Chrome|Chromium|CriOS|Edg|OPR|Firefox|FxiOS/;

describe('needsNotificationKeeper', () => {
  it('never throws, whatever the user agent', () => {
    fc.assert(
      fc.property(fc.string({ unit: 'binary' }), (userAgent) => {
        expect(typeof needsNotificationKeeper(userAgent)).toBe('boolean');
      }),
    );
  });

  it('is false for anything on an iPhone, iPad or iPod, whatever else it claims', () => {
    fc.assert(
      fc.property(
        filler,
        fc.constantFrom('iPhone', 'iPad', 'iPod'),
        filler,
        fc.constantFrom('', 'Chrome/120.0', 'CriOS/120.0', 'Firefox/120.0', 'FxiOS/120.0', 'Safari/605.1'),
        (before, device, after, engine) => {
          expect(needsNotificationKeeper(`${before}${device}${after} ${engine}`)).toBe(false);
        },
      ),
    );
  });

  it('is true for any non Apple mobile agent naming an engine other than WebKit', () => {
    fc.assert(
      fc.property(
        filler.filter((text) => !engineToken.test(text)),
        fc.constantFrom('Chrome/120.0', 'Chromium/120.0', 'Edg/120.0', 'OPR/100.0', 'Firefox/120.0'),
        fc.boolean(),
        (text, engine, withSafari) => {
          // Chrome, Edge and Opera all add "Safari/" to their user agents, which must not fool the check.
          const userAgent = `Mozilla/5.0 (${text}) ${engine}${withSafari ? ' Safari/537.36' : ''}`;
          expect(needsNotificationKeeper(userAgent)).toBe(true);
        },
      ),
    );
  });

  it('is false for Safari on a Mac and true for an agent that names no engine at all', () => {
    fc.assert(
      fc.property(filler.filter((text) => !engineToken.test(text)), (text) => {
        expect(needsNotificationKeeper(`Mozilla/5.0 (Macintosh; ${text}) Version/17.0 Safari/605.1.15`)).toBe(
          false,
        );
        expect(needsNotificationKeeper(`Mozilla/5.0 (${text})`)).toBe(true);
      }),
    );
  });
});
