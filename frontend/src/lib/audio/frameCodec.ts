/**
 * Binary frame decoding, the mirror of `backend/src/ws/protocol.rs`.
 *
 * The header layout is fixed at 24 bytes so it can be read with a handful of `DataView` calls rather than
 * parsed, which matters when this runs ten times a second for the whole time a page is open.
 */

import type { AudioFrame } from '@/api/types';

export const HEADER_BYTES = 24;

/** The ASCII bytes `OAR1` read as a little endian unsigned 32 bit integer. */
export const MAGIC = 0x3152414f;

export const PROTOCOL_VERSION = 1;

const FLAG_LIVE = 0b0000_0001;

/** Signed 16 bit full scale, used to normalise samples into the range Web Audio expects. */
const I16_SCALE = 32768;

/**
 * Decode one binary WebSocket message.
 *
 * Returns `null` rather than throwing for anything unrecognised: a stray message from a proxy or a future
 * protocol version should drop one frame, not tear the connection down.
 */
export function decodeAudioFrame(buffer: ArrayBuffer): AudioFrame | null {
  if (buffer.byteLength < HEADER_BYTES) {
    return null;
  }

  const view = new DataView(buffer);
  if (view.getUint32(0, true) !== MAGIC) {
    return null;
  }
  if (view.getUint8(4) !== PROTOCOL_VERSION) {
    return null;
  }

  const channels = view.getUint8(6);
  const flags = view.getUint8(7);
  const sampleRate = view.getUint32(8, true);
  const sampleCount = view.getUint32(12, true);
  const timestampMs = Number(view.getBigInt64(16, true));

  if (sampleRate === 0 || sampleCount === 0) {
    return null;
  }

  const payload = new Int16Array(buffer, HEADER_BYTES, Math.min(
    sampleCount * Math.max(channels, 1),
    (buffer.byteLength - HEADER_BYTES) >> 1,
  ));

  const samples = new Float32Array(payload.length);
  for (let index = 0; index < payload.length; index += 1) {
    samples[index] = payload[index] / I16_SCALE;
  }

  return {
    timestampMs,
    sampleRate,
    channels: Math.max(channels, 1),
    live: (flags & FLAG_LIVE) !== 0,
    samples,
  };
}
