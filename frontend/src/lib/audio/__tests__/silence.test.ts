import { describe, expect, it } from 'vitest';

import { needsNotificationKeeper, SILENCE_SECONDS, silentWav } from '../silence';

describe('silentWav', () => {
  it('writes a valid mono 8 bit PCM header', () => {
    const wav = silentWav(1, 8000);
    const view = new DataView(wav.buffer);
    const text = (from: number, to: number) => String.fromCharCode(...wav.slice(from, to));

    expect(text(0, 4)).toBe('RIFF');
    expect(text(8, 12)).toBe('WAVE');
    expect(text(12, 16)).toBe('fmt ');
    expect(view.getUint16(20, true)).toBe(1);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint32(24, true)).toBe(8000);
    expect(view.getUint16(34, true)).toBe(8);
    expect(text(36, 40)).toBe('data');
    expect(view.getUint32(40, true)).toBe(8000);
    expect(view.getUint32(4, true)).toBe(wav.length - 8);
  });

  it('is silence, which for unsigned 8 bit is 128', () => {
    const wav = silentWav(0.01, 8000);
    expect([...wav.slice(44)].every((sample) => sample === 128)).toBe(true);
  });

  it('lasts long enough for Chrome to call it real media', () => {
    expect(SILENCE_SECONDS).toBeGreaterThanOrEqual(5);
    expect(silentWav().length - 44).toBe(SILENCE_SECONDS * 8000);
  });
});

const UA = {
  androidChrome:
    'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36',
  iphoneChrome:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/128.0 Mobile/15E148 Safari/604.1',
  iphoneSafari:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Mobile/15E148 Safari/604.1',
  ipadFirefox:
    'Mozilla/5.0 (iPad; CPU OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/130.0 Mobile/15E148 Safari/605.1.15',
  macSafari:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Safari/605.1.15',
  macChrome:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36',
  windowsEdge:
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36 Edg/128.0.0.0',
  linuxFirefox: 'Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0',
};

describe('needsNotificationKeeper', () => {
  it('is needed by Chrome, Edge and Firefox, on phones and computers', () => {
    for (const userAgent of [UA.androidChrome, UA.macChrome, UA.windowsEdge, UA.linuxFirefox]) {
      expect(needsNotificationKeeper(userAgent)).toBe(true);
    }
  });

  it('is not needed by WebKit, which shows controls for the stream itself', () => {
    for (const userAgent of [UA.iphoneChrome, UA.iphoneSafari, UA.ipadFirefox, UA.macSafari]) {
      expect(needsNotificationKeeper(userAgent)).toBe(false);
    }
  });
});
