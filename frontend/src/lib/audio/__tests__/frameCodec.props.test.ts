/**
 * Properties of the binary frame decoder, checked against generated messages rather than hand picked ones.
 *
 * The decoder reads whatever arrives on the socket, so the examples in `frameCodec.test.ts` cannot cover
 * the space that matters: every length, every byte, headers that lie about their sample count. These
 * properties pin the two promises the rest of the audio path relies on. Anything at all decodes to `null`
 * or a well formed frame without throwing, and a frame written to the layout in `backend/src/ws/protocol.rs`
 * reads back field for field.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { decodeAudioFrame, HEADER_BYTES, MAGIC, PROTOCOL_VERSION } from '../frameCodec';

type Fields = {
  reserved: number;
  channels: number;
  flags: number;
  sampleRate: number;
  sampleCount: number;
  timestampMs: bigint;
  samples: number[];
};

/**
 * Write a message byte by byte through `DataView` with explicit little endian calls, the documented
 * layout, rather than through a typed array whose byte order is whatever the host's is.
 */
function encode(fields: Fields): ArrayBuffer {
  const buffer = new ArrayBuffer(HEADER_BYTES + fields.samples.length * 2);
  const view = new DataView(buffer);
  view.setUint32(0, MAGIC, true);
  view.setUint8(4, PROTOCOL_VERSION);
  view.setUint8(5, fields.reserved);
  view.setUint8(6, fields.channels);
  view.setUint8(7, fields.flags);
  view.setUint32(8, fields.sampleRate, true);
  view.setUint32(12, fields.sampleCount, true);
  view.setBigInt64(16, fields.timestampMs, true);
  fields.samples.forEach((sample, index) => {
    view.setInt16(HEADER_BYTES + index * 2, sample, true);
  });
  return buffer;
}

const sample = fc.integer({ min: -32768, max: 32767 });

/** A timestamp a JavaScript number carries exactly, which covers every real recording by a wide margin. */
const safeTimestamp = fc.bigInt({
  min: BigInt(Number.MIN_SAFE_INTEGER),
  max: BigInt(Number.MAX_SAFE_INTEGER),
});

/** A well formed frame whose header tells the truth about its payload. */
const honestFrame = fc
  .record({
    reserved: fc.integer({ min: 0, max: 255 }),
    channels: fc.integer({ min: 1, max: 8 }),
    flags: fc.integer({ min: 0, max: 255 }),
    sampleRate: fc.integer({ min: 1, max: 0xffff_ffff }),
    perChannel: fc.integer({ min: 1, max: 64 }),
    timestampMs: safeTimestamp,
  })
  .chain((header) =>
    fc
      .array(sample, {
        minLength: header.perChannel * header.channels,
        maxLength: header.perChannel * header.channels,
      })
      .map(
        (samples): Fields => ({
          reserved: header.reserved,
          channels: header.channels,
          flags: header.flags,
          sampleRate: header.sampleRate,
          sampleCount: header.perChannel,
          timestampMs: header.timestampMs,
          samples,
        }),
      ),
  );

/**
 * Anything a socket could deliver: pure noise, and noise with a valid magic and version patched in so the
 * generator reaches the code past the first two checks instead of being rejected there every time.
 */
const anyMessage = fc.oneof(
  fc.uint8Array({ maxLength: 96 }),
  fc.uint8Array({ minLength: 5, maxLength: 96 }).map((bytes) => {
    const patched = new Uint8Array(bytes);
    new DataView(patched.buffer).setUint32(0, MAGIC, true);
    patched[4] = PROTOCOL_VERSION;
    return patched;
  }),
  fc.uint8Array({ minLength: HEADER_BYTES, maxLength: 96 }).map((bytes) => {
    const patched = new Uint8Array(bytes);
    const view = new DataView(patched.buffer);
    view.setUint32(0, MAGIC, true);
    patched[4] = PROTOCOL_VERSION;
    // A small claimed count, so both the lying-high and lying-low cases turn up often.
    view.setUint32(12, patched[12] % 40, true);
    return patched;
  }),
);

function toBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

describe('decodeAudioFrame on arbitrary input', () => {
  it('never throws, and returns null or a well formed frame', () => {
    fc.assert(
      fc.property(anyMessage, (bytes) => {
        const buffer = toBuffer(bytes);
        const frame = decodeAudioFrame(buffer);
        if (frame === null) {
          return;
        }

        const view = new DataView(buffer);
        const claimed = view.getUint32(12, true) * Math.max(view.getUint8(6), 1);
        const available = Math.floor((buffer.byteLength - HEADER_BYTES) / 2);

        expect(frame.sampleRate).toBeGreaterThan(0);
        expect(frame.channels).toBeGreaterThanOrEqual(1);
        expect(Number.isInteger(frame.timestampMs)).toBe(true);
        // A header that overstates its payload is cut to what arrived; one that understates it is believed.
        expect(frame.samples.length).toBe(Math.min(claimed, available));
        for (const value of frame.samples) {
          expect(value).toBeGreaterThanOrEqual(-1);
          expect(value).toBeLessThan(1);
        }
      }),
    );
  });

  it('rejects anything shorter than a header', () => {
    fc.assert(
      fc.property(fc.uint8Array({ maxLength: HEADER_BYTES - 1 }), (bytes) => {
        expect(decodeAudioFrame(toBuffer(bytes))).toBeNull();
      }),
    );
  });

  it('rejects a wrong magic or version whatever follows', () => {
    fc.assert(
      fc.property(
        honestFrame,
        fc.integer({ min: 0, max: 0xffff_ffff }).filter((magic) => magic !== MAGIC),
        fc.integer({ min: 0, max: 255 }).filter((version) => version !== PROTOCOL_VERSION),
        (fields, magic, version) => {
          const wrongMagic = encode(fields);
          new DataView(wrongMagic).setUint32(0, magic, true);
          expect(decodeAudioFrame(wrongMagic)).toBeNull();

          const wrongVersion = encode(fields);
          new DataView(wrongVersion).setUint8(4, version);
          expect(decodeAudioFrame(wrongVersion)).toBeNull();
        },
      ),
    );
  });

  it('rejects a zero sample rate or a zero sample count', () => {
    fc.assert(
      fc.property(honestFrame, fc.constantFrom(8, 12), (fields, offset) => {
        const buffer = encode(fields);
        new DataView(buffer).setUint32(offset, 0, true);
        expect(decodeAudioFrame(buffer)).toBeNull();
      }),
    );
  });
});

describe('decodeAudioFrame on frames written to the documented layout', () => {
  it('reads back exactly the fields that were written', () => {
    fc.assert(
      fc.property(honestFrame, (fields) => {
        const frame = decodeAudioFrame(encode(fields));

        expect(frame).not.toBeNull();
        expect(frame?.timestampMs).toBe(Number(fields.timestampMs));
        expect(frame?.sampleRate).toBe(fields.sampleRate);
        expect(frame?.channels).toBe(fields.channels);
        expect(frame?.live).toBe((fields.flags & 1) === 1);
        // Every i16 divided by 32768 is exact in a float32, so this is equality, not closeness.
        expect(Array.from(frame?.samples ?? [])).toEqual(fields.samples.map((value) => value / 32768));
      }),
    );
  });

  it('ignores the reserved byte and every flag bit but the live one', () => {
    fc.assert(
      fc.property(
        honestFrame,
        fc.integer({ min: 0, max: 255 }),
        fc.integer({ min: 0, max: 127 }),
        (fields, reserved, otherFlags) => {
          const altered = { ...fields, reserved, flags: (otherFlags << 1) | (fields.flags & 1) };
          expect(decodeAudioFrame(encode(altered))).toEqual(decodeAudioFrame(encode(fields)));
        },
      ),
    );
  });

  it('treats a channel count of zero as mono', () => {
    fc.assert(
      fc.property(honestFrame, (fields) => {
        const mono = { ...fields, channels: 0, sampleCount: fields.samples.length };
        const frame = decodeAudioFrame(encode(mono));
        expect(frame?.channels).toBe(1);
        expect(frame?.samples.length).toBe(fields.samples.length);
      }),
    );
  });

  it('drops payload beyond what the header claims, and cuts a claim beyond the payload', () => {
    fc.assert(
      fc.property(honestFrame, fc.integer({ min: 1, max: 200 }), (fields, claimed) => {
        const frame = decodeAudioFrame(encode({ ...fields, sampleCount: claimed }));
        const expected = Math.min(claimed * fields.channels, fields.samples.length);
        expect(Array.from(frame?.samples ?? [])).toEqual(
          fields.samples.slice(0, expected).map((value) => value / 32768),
        );
      }),
    );
  });
});
