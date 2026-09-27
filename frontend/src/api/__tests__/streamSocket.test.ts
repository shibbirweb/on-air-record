/**
 * The audio socket wrapper: where it connects, how it sorts text from binary messages, the keep alive,
 * and the reconnection backoff that lets an unattended page recover when the service restarts.
 *
 * The browser socket is replaced by a fake the test drives by hand (open it, deliver a message, drop it),
 * and time is faked, so the backoff schedule and the keep alive interval can be asserted to the
 * millisecond rather than waited for.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { HEADER_BYTES, MAGIC, PROTOCOL_VERSION } from '@/lib/audio/frameCodec';

import { StreamSocket } from '../streamSocket';
import type { StreamSocketHandlers } from '../streamSocket';

class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: FakeWebSocket[] = [];

  readonly url: string;
  readyState = FakeWebSocket.CONNECTING;
  binaryType = 'blob';
  sent: string[] = [];
  closeCalls = 0;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string | ArrayBuffer }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.instances.push(this);
  }

  send(data: string) {
    this.sent.push(data);
  }

  /** What the page asks for. A browser fires `close` later, which the test does with `drop`. */
  close() {
    this.closeCalls += 1;
    this.readyState = FakeWebSocket.CLOSING;
  }

  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.();
  }

  receive(data: string | ArrayBuffer) {
    this.onmessage?.({ data });
  }

  drop() {
    this.readyState = FakeWebSocket.CLOSED;
    this.onerror?.();
    this.onclose?.();
  }
}

function latest(): FakeWebSocket {
  const socket = FakeWebSocket.instances.at(-1);
  if (!socket) {
    throw new Error('no socket was created');
  }
  return socket;
}

/** One wire frame, laid out as `backend/src/ws/protocol.rs` writes it. */
function frameBytes(samples: number[], timestampMs: number): ArrayBuffer {
  const buffer = new ArrayBuffer(HEADER_BYTES + samples.length * 2);
  const view = new DataView(buffer);
  view.setUint32(0, MAGIC, true);
  view.setUint8(4, PROTOCOL_VERSION);
  view.setUint8(6, 1);
  view.setUint8(7, 1);
  view.setUint32(8, 48_000, true);
  view.setUint32(12, samples.length, true);
  view.setBigInt64(16, BigInt(timestampMs), true);
  new Int16Array(buffer, HEADER_BYTES, samples.length).set(samples);
  return buffer;
}

function handlers() {
  return {
    onFrame: vi.fn(),
    onMessage: vi.fn(),
    onOpen: vi.fn(),
    onClose: vi.fn(),
  } satisfies StreamSocketHandlers;
}

function stubWindow(protocol: string) {
  // Timers are looked up at call time, so the fake timers installed by the test are the ones used.
  vi.stubGlobal('window', {
    location: { protocol, host: 'recorder.local:8080' },
    setTimeout: (callback: () => void, delay: number) => setTimeout(callback, delay),
    clearTimeout: (handle: number) => clearTimeout(handle),
    setInterval: (callback: () => void, delay: number) => setInterval(callback, delay),
    clearInterval: (handle: number) => clearInterval(handle),
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_757_034_000_000);
  FakeWebSocket.instances = [];
  vi.stubGlobal('WebSocket', FakeWebSocket);
  stubWindow('http:');
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('connecting', () => {
  it('opens the stream on the page host, asking for binary as array buffers', () => {
    new StreamSocket(handlers()).connect();
    expect(latest().url).toBe('ws://recorder.local:8080/api/ws/stream');
    expect(latest().binaryType).toBe('arraybuffer');
  });

  it('uses a secure socket when the page is served over https', () => {
    stubWindow('https:');
    new StreamSocket(handlers()).connect();
    expect(latest().url).toBe('wss://recorder.local:8080/api/ws/stream');
  });

  it('does not open a second socket while one is connecting or open', () => {
    const socket = new StreamSocket(handlers());
    socket.connect();
    socket.connect();
    latest().open();
    socket.connect();
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it('reports connected only while the socket is open', () => {
    const socket = new StreamSocket(handlers());
    expect(socket.connected).toBe(false);
    socket.connect();
    expect(socket.connected).toBe(false);
    latest().open();
    expect(socket.connected).toBe(true);
    latest().drop();
    expect(socket.connected).toBe(false);
  });

  it('tells the page when the socket opens and when it closes', () => {
    const events = handlers();
    new StreamSocket(events).connect();
    latest().open();
    expect(events.onOpen).toHaveBeenCalledTimes(1);
    latest().drop();
    expect(events.onClose).toHaveBeenCalledTimes(1);
  });

  it('leaves an error to the close that follows it', () => {
    const events = handlers();
    new StreamSocket(events).connect();
    latest().onerror?.();
    expect(events.onClose).not.toHaveBeenCalled();
    expect(FakeWebSocket.instances).toHaveLength(1);
  });
});

describe('messages', () => {
  it('parses a text message as JSON control', () => {
    const events = handlers();
    new StreamSocket(events).connect();
    latest().open();
    latest().receive(JSON.stringify({ type: 'level', rms: 0.1, peak: 0.4 }));
    expect(events.onMessage).toHaveBeenCalledWith({ type: 'level', rms: 0.1, peak: 0.4 });
  });

  it('ignores a text message that is not JSON, and keeps going', () => {
    const events = handlers();
    new StreamSocket(events).connect();
    latest().open();
    latest().receive('{not json');
    latest().receive(JSON.stringify({ type: 'listeners-hidden' }));
    expect(events.onMessage).toHaveBeenCalledTimes(1);
    expect(events.onMessage).toHaveBeenCalledWith({ type: 'listeners-hidden' });
  });

  it('decodes a binary message into an audio frame', () => {
    const events = handlers();
    new StreamSocket(events).connect();
    latest().open();
    latest().receive(frameBytes([16384, -32768], 1_757_034_000_100));

    expect(events.onFrame).toHaveBeenCalledTimes(1);
    const frame = events.onFrame.mock.calls[0][0];
    expect(frame.timestampMs).toBe(1_757_034_000_100);
    expect(Array.from(frame.samples)).toEqual([0.5, -1]);
    expect(events.onMessage).not.toHaveBeenCalled();
  });

  it('drops a binary message that is not a frame', () => {
    const events = handlers();
    new StreamSocket(events).connect();
    latest().open();
    latest().receive(new ArrayBuffer(4));
    expect(events.onFrame).not.toHaveBeenCalled();
  });
});

describe('send', () => {
  it('serialises the message as JSON while the socket is open', () => {
    const socket = new StreamSocket(handlers());
    socket.connect();
    latest().open();
    socket.send({ type: 'seek', timestampMs: 42 });
    expect(latest().sent).toEqual([JSON.stringify({ type: 'seek', timestampMs: 42 })]);
  });

  it('drops the message before the socket opens, and when there is no socket', () => {
    const socket = new StreamSocket(handlers());
    socket.send({ type: 'live' });
    socket.connect();
    socket.send({ type: 'live' });
    expect(latest().sent).toEqual([]);
  });
});

describe('reconnection', () => {
  it('backs off exponentially from half a second, capped at eight', () => {
    new StreamSocket(handlers()).connect();
    const delays: number[] = [];

    for (let attempt = 0; attempt < 7; attempt += 1) {
      const before = FakeWebSocket.instances.length;
      latest().drop();
      let waited = 0;
      while (FakeWebSocket.instances.length === before) {
        vi.advanceTimersByTime(1);
        waited += 1;
      }
      delays.push(waited);
    }

    expect(delays).toEqual([500, 1000, 2000, 4000, 8000, 8000, 8000]);
  });

  it('starts the backoff again after a connection succeeds', () => {
    new StreamSocket(handlers()).connect();
    latest().drop();
    vi.advanceTimersByTime(500);
    latest().drop();
    vi.advanceTimersByTime(1000);
    latest().open();
    latest().drop();

    vi.advanceTimersByTime(499);
    expect(FakeWebSocket.instances).toHaveLength(3);
    vi.advanceTimersByTime(1);
    expect(FakeWebSocket.instances).toHaveLength(4);
  });

  it('does not reconnect after the page closes it', () => {
    const events = handlers();
    const socket = new StreamSocket(events);
    socket.connect();
    latest().open();
    socket.close();
    expect(latest().closeCalls).toBe(1);
    expect(socket.connected).toBe(false);

    latest().drop();
    vi.advanceTimersByTime(60_000);
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it('cancels a reconnect that was already waiting when closed', () => {
    const socket = new StreamSocket(handlers());
    socket.connect();
    latest().drop();
    socket.close();
    vi.advanceTimersByTime(60_000);
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it('can be connected again after being closed', () => {
    const socket = new StreamSocket(handlers());
    socket.connect();
    latest().drop();
    socket.close();
    socket.connect();
    expect(FakeWebSocket.instances).toHaveLength(2);
    // A drop of the new socket reconnects as normal, since the page wants it open again.
    latest().drop();
    vi.advanceTimersByTime(8_000);
    expect(FakeWebSocket.instances).toHaveLength(3);
  });

  it('pays no attention to the close of a socket it replaced', () => {
    const events = handlers();
    const socket = new StreamSocket(events);
    socket.connect();
    const first = latest();
    first.open();
    socket.close();
    socket.connect();
    const second = latest();
    second.open();
    events.onClose.mockClear();

    // The browser finishes closing the first socket only now.
    first.drop();
    expect(events.onClose).not.toHaveBeenCalled();
    expect(socket.connected).toBe(true);
    vi.advanceTimersByTime(60_000);
    expect(FakeWebSocket.instances).toHaveLength(2);

    socket.send({ type: 'live' });
    expect(second.sent).toContain(JSON.stringify({ type: 'live' }));
  });

  it('reports no disconnect when a socket the page closed finally closes', () => {
    const events = handlers();
    const socket = new StreamSocket(events);
    socket.connect();
    latest().open();
    socket.close();
    latest().drop();
    // The page tore it down on purpose, and a newer socket elsewhere may already own the connection state.
    expect(events.onClose).not.toHaveBeenCalled();
  });

  it('delivers nothing that arrives on a socket after the page closed it', () => {
    const events = handlers();
    const socket = new StreamSocket(events);
    socket.connect();
    const closed = latest();
    closed.open();
    socket.close();

    closed.receive(frameBytes([1_000, -1_000], 1_000));
    closed.receive(JSON.stringify({ type: 'switched-to-live' }));
    expect(events.onFrame).not.toHaveBeenCalled();
    expect(events.onMessage).not.toHaveBeenCalled();
  });
});

describe('keep alive', () => {
  it('pings every fifteen seconds while open, with the client clock', () => {
    new StreamSocket(handlers()).connect();
    latest().open();

    vi.advanceTimersByTime(14_999);
    expect(latest().sent).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(latest().sent).toEqual([JSON.stringify({ type: 'ping', clientTimeMs: 1_757_034_015_000 })]);
    vi.advanceTimersByTime(15_000);
    expect(latest().sent).toHaveLength(2);
  });

  it('does not ping before the socket opens', () => {
    new StreamSocket(handlers()).connect();
    vi.advanceTimersByTime(60_000);
    expect(latest().sent).toEqual([]);
  });

  it('stops pinging when the socket drops', () => {
    new StreamSocket(handlers()).connect();
    const first = latest();
    first.open();
    first.drop();
    vi.advanceTimersByTime(60_000);
    expect(first.sent).toEqual([]);
  });

  it('stops pinging when the page closes the socket', () => {
    const socket = new StreamSocket(handlers());
    socket.connect();
    const first = latest();
    first.open();
    socket.close();
    vi.advanceTimersByTime(60_000);
    expect(first.sent).toEqual([]);
  });

  it('runs one keep alive, not one per open, across reconnects', () => {
    new StreamSocket(handlers()).connect();
    latest().open();
    latest().drop();
    vi.advanceTimersByTime(500);
    latest().open();

    vi.advanceTimersByTime(15_000);
    expect(latest().sent).toHaveLength(1);
  });
});
