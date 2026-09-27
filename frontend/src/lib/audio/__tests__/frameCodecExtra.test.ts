/**
 * Frame decoding cases `frameCodec.test.ts` leaves out: stereo payloads, a header claiming no channels,
 * flag bits other than live, a payload longer than the header says, and the protocol constants, which
 * must stay equal to `backend/src/ws/protocol.rs`.
 */

import { describe, expect, it } from 'vitest';

import { decodeAudioFrame, HEADER_BYTES, MAGIC, PROTOCOL_VERSION } from '../frameCodec';

function encode(options: { channels: number; flags?: number; sampleCount: number; samples: number[]; timestampMs?: number }) {
  const buffer = new ArrayBuffer(HEADER_BYTES + options.samples.length * 2);
  const view = new DataView(buffer);
  view.setUint32(0, MAGIC, true);
  view.setUint8(4, PROTOCOL_VERSION);
  view.setUint8(6, options.channels);
  view.setUint8(7, options.flags ?? 1);
  view.setUint32(8, 48_000, true);
  view.setUint32(12, options.sampleCount, true);
  view.setBigInt64(16, BigInt(options.timestampMs ?? 0), true);
  new Int16Array(buffer, HEADER_BYTES, options.samples.length).set(options.samples);
  return buffer;
}

describe('protocol constants', () => {
  it('match the backend header layout', () => {
    expect(HEADER_BYTES).toBe(24);
    expect(PROTOCOL_VERSION).toBe(1);
    // "OAR1" as little endian bytes.
    const bytes = new Uint8Array(new Uint32Array([MAGIC]).buffer);
    expect(String.fromCharCode(...bytes)).toBe('OAR1');
  });
});

describe('decodeAudioFrame, remaining cases', () => {
  it('reads a stereo payload of sample count times channels', () => {
    const frame = decodeAudioFrame(encode({ channels: 2, sampleCount: 2, samples: [16384, -16384, 8192, -8192] }));
    expect(frame?.channels).toBe(2);
    expect(Array.from(frame?.samples ?? [])).toEqual([0.5, -0.5, 0.25, -0.25]);
  });

  it('treats a header claiming no channels as mono', () => {
    const frame = decodeAudioFrame(encode({ channels: 0, sampleCount: 2, samples: [1, 2] }));
    expect(frame?.channels).toBe(1);
    expect(frame?.samples).toHaveLength(2);
  });

  it('reads the live flag from its own bit, ignoring the others', () => {
    expect(decodeAudioFrame(encode({ channels: 1, flags: 0b1111_1110, sampleCount: 1, samples: [1] }))?.live).toBe(false);
    expect(decodeAudioFrame(encode({ channels: 1, flags: 0b1000_0001, sampleCount: 1, samples: [1] }))?.live).toBe(true);
  });

  it('reads no more samples than the header announces', () => {
    const frame = decodeAudioFrame(encode({ channels: 1, sampleCount: 2, samples: [1, 2, 3, 4] }));
    expect(frame?.samples).toHaveLength(2);
  });

  it('keeps a millisecond timestamp exact', () => {
    const frame = decodeAudioFrame(encode({ channels: 1, sampleCount: 1, samples: [0], timestampMs: 1_757_034_000_123 }));
    expect(frame?.timestampMs).toBe(1_757_034_000_123);
  });

  it('accepts a header with no payload bytes at all as long as it announces samples, yielding none', () => {
    const frame = decodeAudioFrame(encode({ channels: 1, sampleCount: 5, samples: [] }));
    expect(frame?.samples).toHaveLength(0);
  });
});
