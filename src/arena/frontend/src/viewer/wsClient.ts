/**
 * wsClient — live WebSocket connection to the Arena's /observe stream.
 *
 * Separates the transport concern from the renderer.  The renderer takes
 * a parsed GodViewFrame; this module owns the socket and calls back with
 * parsed frames.
 *
 * Seam for issue 07 (replay): the replay loader feeds the same callback
 * shape without touching this module.
 */

import { parseGodViewFrame, type GodViewFrame } from "../lib/frameParser.ts";

export type FrameCallback = (frame: GodViewFrame) => void;
export type StatusCallback = (message: string) => void;

export interface ObserverClientOptions {
  /** Full WS URL to the observer stream, e.g. "ws://localhost:3000/observe" */
  url: string;
  onFrame: FrameCallback;
  onStatus?: StatusCallback;
  /** Milliseconds before attempting reconnect after disconnect. Default 2000. */
  reconnectDelayMs?: number;
}

/**
 * A self-reconnecting WebSocket client for the Arena's /observe god-mode stream.
 *
 * Call `connect()` once; the client reconnects automatically on disconnect.
 * Call `disconnect()` to stop.
 */
export class ObserverClient {
  private socket: WebSocket | null = null;
  private stopped = false;
  private readonly opts: Required<ObserverClientOptions>;

  constructor(opts: ObserverClientOptions) {
    this.opts = {
      reconnectDelayMs: 2000,
      onStatus: () => {},
      ...opts,
    };
  }

  connect(): void {
    this.stopped = false;
    this.openSocket();
  }

  disconnect(): void {
    this.stopped = true;
    this.socket?.close();
    this.socket = null;
  }

  private openSocket(): void {
    if (this.stopped) return;

    this.opts.onStatus(`connecting to ${this.opts.url}…`);
    const ws = new WebSocket(this.opts.url);
    this.socket = ws;

    ws.addEventListener("open", () => {
      this.opts.onStatus("connected — receiving god-mode stream");
    });

    ws.addEventListener("message", (ev: MessageEvent<string>) => {
      let raw: unknown;
      try {
        raw = JSON.parse(ev.data);
      } catch {
        return; // malformed JSON — silently ignore
      }

      const frame = parseGodViewFrame(raw);
      if (frame) {
        this.opts.onFrame(frame);
      }
    });

    ws.addEventListener("close", () => {
      if (this.stopped) return;
      this.opts.onStatus(
        `disconnected — reconnecting in ${this.opts.reconnectDelayMs}ms…`
      );
      setTimeout(() => this.openSocket(), this.opts.reconnectDelayMs);
    });

    ws.addEventListener("error", () => {
      // The close event fires after error; reconnect is handled there.
      this.opts.onStatus("connection error");
    });
  }
}

/**
 * Derive the observer WS URL from the current page's location.
 *
 * In dev (vite proxy not configured), falls back to ws://localhost:3000/observe.
 * Override by setting `window.ARENA_WS_URL` before the script loads.
 */
export function defaultObserverUrl(): string {
  // Allow per-deployment override via global
  const win = window as typeof window & { ARENA_WS_URL?: string };
  if (win.ARENA_WS_URL) return win.ARENA_WS_URL;

  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/observe`;
}
