/** Formatting helpers shared by the timeline, the status panel and the transport bar. */

const TIME_FORMAT = new Intl.DateTimeFormat(undefined, {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
});

const DATE_TIME_FORMAT = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
});

/** `14:23:05`, the label used on the timeline and the playhead. */
export function formatClock(timestampMs: number | null | undefined): string {
  if (timestampMs === null || timestampMs === undefined || !Number.isFinite(timestampMs)) {
    return '--:--:--';
  }
  return TIME_FORMAT.format(new Date(timestampMs));
}

/** `Sep 5, 14:23:05`, used where the day matters. */
export function formatDateTime(timestampMs: number | null | undefined): string {
  if (timestampMs === null || timestampMs === undefined || !Number.isFinite(timestampMs)) {
    return 'unknown';
  }
  return DATE_TIME_FORMAT.format(new Date(timestampMs));
}

/** `01:02:03` for an elapsed span. */
export function formatDuration(durationMs: number): string {
  const totalSeconds = Math.max(Math.floor(durationMs / 1000), 0);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const pad = (value: number) => value.toString().padStart(2, '0');
  return `${pad(hours)}:${pad(minutes)}:${pad(seconds)}`;
}

/** `2m 14s behind live`, or `live` when the offset is inside the jitter buffer. */
export function formatOffsetFromLive(offsetMs: number): string {
  if (offsetMs < 1500) {
    return 'live';
  }
  const seconds = Math.round(offsetMs / 1000);
  if (seconds < 60) {
    return `${seconds}s behind`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ${seconds % 60}s behind`;
  }
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m behind`;
}

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return '0 B';
  }
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), BYTE_UNITS.length - 1);
  const value = bytes / 1024 ** exponent;
  return `${value.toFixed(exponent === 0 ? 0 : 1)} ${BYTE_UNITS[exponent]}`;
}

/** Convert a normalised amplitude to dBFS for the meter scale. */
export function toDecibels(amplitude: number): number {
  if (amplitude <= 0.00001) {
    return -100;
  }
  return 20 * Math.log10(amplitude);
}

/**
 * Map a normalised amplitude onto `0..1` for drawing.
 *
 * Meters are drawn on a decibel scale rather than a linear one, because linear amplitude spends almost
 * all of its range on the loudest few decibels and speech at a normal level barely moves the bar.
 */
export function meterScale(amplitude: number, floorDb = -60): number {
  const decibels = toDecibels(amplitude);
  if (decibels <= floorDb) {
    return 0;
  }
  return Math.min((decibels - floorDb) / -floorDb, 1);
}

/**
 * Bytes an hour of recording occupies at `sampleRate`.
 *
 * Mono, 16 bit, uncompressed, matching `bytes_per_hour` in `backend/src/dto/session_dto.rs`. Because
 * nothing is compressed this is exact arithmetic, not an estimate, which is what lets the settings page
 * promise a specific number of gigabytes.
 */
export function pcmBytesPerHour(sampleRate: number): number {
  if (!Number.isFinite(sampleRate) || sampleRate <= 0) {
    return 0;
  }
  return sampleRate * 2 * 3600;
}

/** Bit rate of an uncompressed mono 16 bit stream, in kbps. */
export function pcmBitRateKbps(sampleRate: number): number {
  if (!Number.isFinite(sampleRate) || sampleRate <= 0) {
    return 0;
  }
  return Math.round((sampleRate * 16) / 1000);
}
