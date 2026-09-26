/**
 * A silent clip for Chrome on Android's media notification.
 *
 * Chrome on Android shows its media notification, and with it the lock screen controls, only for media it
 * considers real content: an element with a known duration of at least five seconds. The engine's output
 * element plays a live MediaStream, which Chrome treats like a video call and never gives a notification,
 * though it does keep it playing with the screen off. Looping this silent clip alongside it gives Chrome
 * something it will show, and the notification then carries the metadata and buttons set through the
 * Media Session API. The iPhone shows controls for the stream itself, so it does not need this.
 */

/** Long enough to clear Chrome's five second threshold with room to spare. */
export const SILENCE_SECONDS = 10;
/** The lowest rate every browser decodes; silence has nothing to lose by it. */
const SILENCE_SAMPLE_RATE = 8000;

/**
 * A mono 8 bit PCM WAV file of silence. Eight bit WAV is unsigned, so silence is 128, not 0. At 8 kHz ten
 * seconds is 80 KB, built once in memory rather than shipped as a file.
 */
export function silentWav(
  seconds: number = SILENCE_SECONDS,
  sampleRate: number = SILENCE_SAMPLE_RATE,
): Uint8Array<ArrayBuffer> {
  const samples = Math.max(Math.round(seconds * sampleRate), 1);
  const bytes = new Uint8Array(new ArrayBuffer(44 + samples));
  const view = new DataView(bytes.buffer);
  const ascii = (offset: number, text: string) => {
    for (let index = 0; index < text.length; index += 1) {
      bytes[offset + index] = text.charCodeAt(index);
    }
  };

  ascii(0, 'RIFF');
  view.setUint32(4, 36 + samples, true);
  ascii(8, 'WAVE');
  ascii(12, 'fmt ');
  view.setUint32(16, 16, true); // size of the format chunk
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate, true); // bytes per second: one byte per sample
  view.setUint16(32, 1, true); // block align
  view.setUint16(34, 8, true); // bits per sample
  ascii(36, 'data');
  view.setUint32(40, samples, true);
  bytes.fill(128, 44);
  return bytes;
}

/** Whether this browser needs the silent clip: Chrome and the others built on it, on Android. */
export function needsNotificationKeeper(userAgent: string): boolean {
  return /Android/i.test(userAgent);
}
