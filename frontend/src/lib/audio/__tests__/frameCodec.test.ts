import { describe, expect, it } from 'vitest';

import { decodeAudioFrame, HEADER_BYTES, MAGIC, PROTOCOL_VERSION } from '../frameCodec';

/** Build a wire message the way `backend/src/ws/protocol.rs` does, so the two stay in step. */
function encodeFrame(options: {
  magic?: number;
  version?: number;
  channels?: number;
  live?: boolean;
  sampleRate?: number;
  samples: number[];
  timestampMs?: number;
}): ArrayBuffer {
  const samples = options.samples;
  const buffer = new ArrayBuffer(HEADER_BYTES + samples.length * 2);
  const view = new DataView(buffer);

  view.setUint32(0, options.magic ?? MAGIC, true);
  view.setUint8(4, options.version ?? PROTOCOL_VERSION);
  view.setUint8(5, 0);
  view.setUint8(6, options.channels ?? 1);
  view.setUint8(7, options.live === false ? 0 : 1);
  view.setUint32(8, options.sampleRate ?? 48_000, true);
  view.setUint32(12, samples.length / (options.channels ?? 1), true);
  view.setBigInt64(16, BigInt(options.timestampMs ?? 1_757_030_400_000), true);

  const payload = new Int16Array(buffer, HEADER_BYTES, samples.length);
  payload.set(samples);

  return buffer;
}

describe('decodeAudioFrame', () => {
  it('reads the header fields', () => {
    const frame = decodeAudioFrame(
      encodeFrame({ samples: [0, 0, 0, 0], timestampMs: 1_757_030_400_000 }),
    );

    expect(frame).not.toBeNull();
    expect(frame?.sampleRate).toBe(48_000);
    expect(frame?.channels).toBe(1);
    expect(frame?.live).toBe(true);
    expect(frame?.timestampMs).toBe(1_757_030_400_000);
    expect(frame?.samples).toHaveLength(4);
  });

  it('normalises samples into the range Web Audio expects', () => {
    const frame = decodeAudioFrame(encodeFrame({ samples: [0, 16384, -32768] }));

    expect(frame?.samples[0]).toBe(0);
    expect(frame?.samples[1]).toBeCloseTo(0.5, 5);
    expect(frame?.samples[2]).toBe(-1);
  });

  it('marks replayed frames as not live', () => {
    const frame = decodeAudioFrame(encodeFrame({ samples: [1, 2], live: false }));
    expect(frame?.live).toBe(false);
  });

  it('rejects a message that is not ours rather than throwing', () => {
    expect(decodeAudioFrame(encodeFrame({ samples: [1], magic: 0xdeadbeef }))).toBeNull();
    expect(decodeAudioFrame(encodeFrame({ samples: [1], version: 99 }))).toBeNull();
    expect(decodeAudioFrame(new ArrayBuffer(8))).toBeNull();
  });

  it('rejects a frame with no audio in it', () => {
    expect(decodeAudioFrame(encodeFrame({ samples: [], sampleRate: 48_000 }))).toBeNull();
    expect(decodeAudioFrame(encodeFrame({ samples: [1, 2], sampleRate: 0 }))).toBeNull();
  });

  it('never reads past the end of a truncated payload', () => {
    const full = encodeFrame({ samples: [1, 2, 3, 4] });
    const truncated = full.slice(0, HEADER_BYTES + 4);

    const frame = decodeAudioFrame(truncated);
    expect(frame?.samples).toHaveLength(2);
  });
});
