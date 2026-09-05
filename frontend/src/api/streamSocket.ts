/**
 * WebSocket transport for the audio stream.
 *
 * Wraps the browser socket with three things the raw API does not give us: binary frame decoding,
 * reconnection with backoff, and a keep alive. Reconnection matters more here than in a typical app,
 * because the backend restarting is a routine event during development and an unattended kiosk showing
 * this page should recover on its own rather than needing a reload.
 */

import { decodeAudioFrame } from '@/lib/audio/frameCodec';

import type { AudioFrame, ClientMessage, ServerMessage } from './types';

export type StreamSocketHandlers = {
  onFrame: (frame: AudioFrame) => void;
  onMessage: (message: ServerMessage) => void;
  onOpen: () => void;
  onClose: () => void;
};

const RECONNECT_BASE_MS = 500;
const RECONNECT_MAX_MS = 8000;
const KEEPALIVE_INTERVAL_MS = 15_000;

function streamUrl(): string {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}/api/ws/stream`;
}

export class StreamSocket {
  private socket: WebSocket | null = null;
  private handlers: StreamSocketHandlers;
  private reconnectAttempts = 0;
  private reconnectTimer: number | null = null;
  private keepaliveTimer: number | null = null;
  private closedByUser = false;

  constructor(handlers: StreamSocketHandlers) {
    this.handlers = handlers;
  }

  get connected(): boolean {
    return this.socket?.readyState === WebSocket.OPEN;
  }

  connect(): void {
    if (this.socket && this.socket.readyState <= WebSocket.OPEN) {
      return;
    }

    this.closedByUser = false;
    const socket = new WebSocket(streamUrl());
    socket.binaryType = 'arraybuffer';
    this.socket = socket;

    socket.onopen = () => {
      this.reconnectAttempts = 0;
      this.startKeepalive();
      this.handlers.onOpen();
    };

    socket.onmessage = (event: MessageEvent<string | ArrayBuffer>) => {
      if (typeof event.data === 'string') {
        try {
          this.handlers.onMessage(JSON.parse(event.data) as ServerMessage);
        } catch {
          // A control message we cannot parse is not worth breaking the stream over.
        }
        return;
      }

      const frame = decodeAudioFrame(event.data);
      if (frame) {
        this.handlers.onFrame(frame);
      }
    };

    socket.onclose = () => {
      this.stopKeepalive();
      this.socket = null;
      this.handlers.onClose();
      if (!this.closedByUser) {
        this.scheduleReconnect();
      }
    };

    socket.onerror = () => {
      // `onclose` always follows, and it is the one that owns the reconnect, so there is nothing to do
      // here beyond letting the error fall through.
    };
  }

  send(message: ClientMessage): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify(message));
    }
  }

  close(): void {
    this.closedByUser = true;
    this.clearReconnect();
    this.stopKeepalive();
    this.socket?.close();
    this.socket = null;
  }

  private scheduleReconnect(): void {
    this.clearReconnect();
    // Exponential backoff so a backend that is down does not turn into a connection storm.
    const delay = Math.min(RECONNECT_BASE_MS * 2 ** this.reconnectAttempts, RECONNECT_MAX_MS);
    this.reconnectAttempts += 1;
    this.reconnectTimer = window.setTimeout(() => this.connect(), delay);
  }

  private clearReconnect(): void {
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private startKeepalive(): void {
    this.stopKeepalive();
    // Idle WebSocket connections are dropped by some proxies after a minute, and a paused session sends
    // nothing at all, so the client keeps the path warm itself.
    this.keepaliveTimer = window.setInterval(() => {
      this.send({ type: 'ping', clientTimeMs: Date.now() });
    }, KEEPALIVE_INTERVAL_MS);
  }

  private stopKeepalive(): void {
    if (this.keepaliveTimer !== null) {
      window.clearInterval(this.keepaliveTimer);
      this.keepaliveTimer = null;
    }
  }
}
