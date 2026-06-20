/**
 * replayPlayer — client-side replay controller for the Shatterbelt Salvagers Viewer.
 *
 * ## Architecture: why we buffer the server's god-view stream
 *
 * The browser contains NO arena engine — it cannot reconstruct frames from a
 * recorded seed + intent log.  Instead the server replays the recording by
 * calling `POST /recordings/{id}/replay`, which drives `run_replay` server-side
 * and publishes every tick's god-view frame to the ObserverHub.  All connected
 * `/observe` WebSocket subscribers receive the replay frames in real time,
 * exactly as they would during a live match.
 *
 * ReplayPlayer exploits this by:
 *   1. Connecting to `/observe` BEFORE triggering the replay.
 *   2. Calling `POST /recordings/{id}/replay`.
 *   3. Buffering each incoming GodViewFrame into `this.buffer[]`.
 *   4. Detecting completion (frames stop arriving, or tick >= maxTicks).
 *   5. Offering client-side play/pause/speed/scrub over the buffered sequence.
 *
 * Because buffering and playback are decoupled the spectator can scrub freely
 * once the buffer is full, without any further server involvement.
 *
 * ## Render pipeline reuse
 *
 * The `onFrame` callback passed to the constructor is identical to the live
 * viewer's `onFrame` — it calls `renderer.renderFrame`, `hud.update`, and
 * `deriveSoundCues` + `sound.playCues` in exactly the same way.  Replays
 * therefore look and sound identical to live matches.
 *
 * ## Timeline math
 *
 * Playback uses `playbackMsToIndex` from `replayTimeline.ts`.  The player
 * tracks `this.playbackMs` as a float (accumulated real-time × speed) and
 * calls `playbackMsToIndex(this.playbackMs, buffer.length)` each RAF step to
 * avoid sub-frame drift.  Scrubbing sets `playbackMs` directly via
 * `indexToPosition` / `positionToIndex`.
 *
 * Seam: consumed by viewer/main.ts (issue 07).
 */

import { parseGodViewFrame, type GodViewFrame } from "../lib/frameParser.ts";
import {
  positionToIndex,
  indexToPosition,
  playbackMsToIndex,
  TICK_RATE,
} from "../lib/replayTimeline.ts";

// ── Public types ──────────────────────────────────────────────────────────────

/** A single item returned by `GET /recordings`. */
export interface RecordingItem {
  matchId: string;
  seed: number;
  tickCount: number;
  winner: string | null;
}

/** Callbacks wired from the viewer's render pipeline. */
export interface ReplayPlayerOptions {
  /** Called with each frame to render + update HUD + play sound (same as live). */
  onFrame: (frame: GodViewFrame) => void;
  /** Called with status messages for the status-bar. */
  onStatus: (msg: string) => void;
  /** Called when buffering completes (buffer is ready for scrub/play). */
  onBuffered?: (frameCount: number) => void;
  /** Called on each RAF step with (currentIndex, bufferLength). */
  onProgress?: (index: number, bufferLength: number) => void;
  /**
   * Base HTTP URL for API calls.  Defaults to "" (same-origin).
   * Override in tests or multi-origin deployments.
   */
  apiBaseUrl?: string;
  /**
   * WebSocket URL for the /observe stream.  Defaults to
   * `defaultObserverUrl()` from wsClient.ts — derived from `location`.
   */
  observeWsUrl?: string;
  /**
   * Milliseconds to wait after the last frame arrives before declaring
   * the buffer complete.  Default 500 ms.
   */
  bufferIdleTimeoutMs?: number;
}

// ── ReplayPlayer ──────────────────────────────────────────────────────────────

/**
 * Controls buffered replay of a recorded match.
 *
 * Usage:
 * ```ts
 * const player = new ReplayPlayer({ onFrame, onStatus, onBuffered, onProgress });
 * const recordings = await ReplayPlayer.listRecordings();
 * await player.load(recordings[0].matchId);
 * player.play();
 * ```
 */
export class ReplayPlayer {
  // ── Configuration ─────────────────────────────────────────────────────────
  private readonly opts: Required<ReplayPlayerOptions>;

  // ── Buffer ────────────────────────────────────────────────────────────────
  private buffer: GodViewFrame[] = [];
  private buffering = false;
  private bufferSocket: WebSocket | null = null;
  private idleTimer: ReturnType<typeof setTimeout> | null = null;

  // ── Playback state ────────────────────────────────────────────────────────
  private playing = false;
  private speed = 1;
  /**
   * Accumulated real-time × speed in ms since the start of the buffer.
   * Maintained as a float to avoid sub-frame drift (see replayTimeline.ts).
   */
  private playbackMs = 0;
  private lastRafTime: number | null = null;
  private rafHandle: number | null = null;
  private currentIndex = 0;
  private prevFrame: GodViewFrame | null = null;

  constructor(opts: ReplayPlayerOptions) {
    this.opts = {
      onBuffered: () => {},
      onProgress: () => {},
      apiBaseUrl: "",
      observeWsUrl: defaultObserveWsUrl(),
      bufferIdleTimeoutMs: 500,
      ...opts,
    };
  }

  // ── Static API ─────────────────────────────────────────────────────────────

  /**
   * Fetch the list of recorded matches from `GET /recordings`.
   *
   * @param apiBaseUrl  Base URL (default: same-origin).
   */
  static async listRecordings(apiBaseUrl = ""): Promise<RecordingItem[]> {
    const res = await fetch(`${apiBaseUrl}/recordings`);
    if (!res.ok) throw new Error(`GET /recordings failed: ${res.status}`);
    return res.json() as Promise<RecordingItem[]>;
  }

  // ── Load ──────────────────────────────────────────────────────────────────

  /**
   * Load a recording by ID.
   *
   * Steps:
   *   1. Resets and clears any previous buffer.
   *   2. Opens a WebSocket to `/observe` and begins buffering incoming frames.
   *   3. POSTs to `/recordings/{matchId}/replay` to trigger server-side replay.
   *   4. Waits for `bufferIdleTimeoutMs` of silence to detect completion.
   *   5. Resolves when buffering is declared complete.
   *
   * @param matchId  Recording ID from `listRecordings()`.
   */
  async load(matchId: string): Promise<void> {
    this.stop();
    this.buffer = [];
    this.playbackMs = 0;
    this.currentIndex = 0;
    this.prevFrame = null;
    this.buffering = true;

    this.opts.onStatus(`Buffering replay ${matchId}…`);

    await new Promise<void>((resolve, reject) => {
      const ws = new WebSocket(this.opts.observeWsUrl);
      this.bufferSocket = ws;

      const scheduleIdle = (): void => {
        if (this.idleTimer !== null) clearTimeout(this.idleTimer);
        this.idleTimer = setTimeout(() => {
          finalize();
        }, this.opts.bufferIdleTimeoutMs);
      };

      const finalize = (): void => {
        if (!this.buffering) return;
        this.buffering = false;
        ws.close();
        this.bufferSocket = null;
        if (this.idleTimer !== null) {
          clearTimeout(this.idleTimer);
          this.idleTimer = null;
        }
        this.opts.onStatus(
          `Buffered ${this.buffer.length} frames — ready to play`,
        );
        this.opts.onBuffered(this.buffer.length);
        resolve();
      };

      ws.addEventListener("open", () => {
        // Trigger the server replay AFTER the WS is open so we don't miss frames
        fetch(`${this.opts.apiBaseUrl}/recordings/${matchId}/replay`, {
          method: "POST",
        }).catch((err) => {
          this.buffering = false;
          ws.close();
          reject(new Error(`POST /recordings/${matchId}/replay failed: ${String(err)}`));
        });
        // Start idle timer in case the recording is empty
        scheduleIdle();
      });

      ws.addEventListener("message", (ev: MessageEvent<string>) => {
        let raw: unknown;
        try {
          raw = JSON.parse(ev.data);
        } catch {
          return;
        }
        const frame = parseGodViewFrame(raw);
        if (!frame) return;

        this.buffer.push(frame);
        this.opts.onStatus(
          `Buffering… ${this.buffer.length} frames`,
        );

        // Reset idle timer on every incoming frame
        scheduleIdle();

        // Also finalize eagerly when we hit maxTicks
        if (frame.tick >= frame.maxTicks) {
          finalize();
        }
      });

      ws.addEventListener("error", () => {
        if (this.buffering) {
          this.buffering = false;
          reject(new Error("WebSocket error while buffering replay"));
        }
      });

      ws.addEventListener("close", () => {
        // If the socket closes cleanly before finalize(), treat it as done
        if (this.buffering) finalize();
      });
    });
  }

  // ── Playback controls ─────────────────────────────────────────────────────

  /** Start or resume playback. */
  play(): void {
    if (this.buffer.length === 0) return;
    this.playing = true;
    this.lastRafTime = null;
    if (this.rafHandle === null) {
      this.rafHandle = requestAnimationFrame(this.rafLoop);
    }
  }

  /** Pause playback without resetting position. */
  pause(): void {
    this.playing = false;
  }

  /** Stop playback and reset to the beginning. */
  stop(): void {
    this.playing = false;
    this.playbackMs = 0;
    this.currentIndex = 0;
    this.prevFrame = null;
    if (this.rafHandle !== null) {
      cancelAnimationFrame(this.rafHandle);
      this.rafHandle = null;
    }
    if (this.bufferSocket) {
      this.bufferSocket.close();
      this.bufferSocket = null;
    }
    if (this.idleTimer !== null) {
      clearTimeout(this.idleTimer);
      this.idleTimer = null;
    }
    this.buffering = false;
  }

  /** Set playback speed multiplier (e.g. 0.5, 1, 2, 4). */
  setSpeed(speed: number): void {
    // Rebase playbackMs so the frame position doesn't jump on speed change
    this.speed = speed;
  }

  /**
   * Scrub to a normalized timeline position [0,1].
   *
   * Shows the frame at that position immediately (even while paused).
   */
  scrubTo(position: number): void {
    if (this.buffer.length === 0) return;
    const idx = positionToIndex(this.buffer.length, position);
    this.jumpToIndex(idx);
  }

  /** Whether playback is currently running. */
  get isPlaying(): boolean {
    return this.playing;
  }

  /** Current playback speed multiplier. */
  get currentSpeed(): number {
    return this.speed;
  }

  /** Current frame index in the buffer. */
  get frameIndex(): number {
    return this.currentIndex;
  }

  /** Number of buffered frames. */
  get frameCount(): number {
    return this.buffer.length;
  }

  /** Current normalized timeline position [0,1]. */
  get position(): number {
    return indexToPosition(this.buffer.length, this.currentIndex);
  }

  // ── Internal RAF loop ──────────────────────────────────────────────────────

  private readonly rafLoop = (timestamp: number): void => {
    this.rafHandle = null;

    if (this.playing && this.buffer.length > 0) {
      if (this.lastRafTime !== null) {
        const dt = timestamp - this.lastRafTime;
        this.playbackMs += dt * this.speed;
      }
      this.lastRafTime = timestamp;

      const newIndex = playbackMsToIndex(this.playbackMs, this.buffer.length);

      if (newIndex !== this.currentIndex || this.currentIndex === 0) {
        this.jumpToIndex(newIndex);
      }

      // Stop at the end
      if (this.currentIndex >= this.buffer.length - 1) {
        this.playing = false;
        this.opts.onStatus("Replay ended");
      }
    }

    // Keep the loop alive while playing
    if (this.playing) {
      this.rafHandle = requestAnimationFrame(this.rafLoop);
    }
  };

  // ── Internal helpers ──────────────────────────────────────────────────────

  /** Jump to a specific buffer index, emit the frame, update progress. */
  private jumpToIndex(idx: number): void {
    const clamped = Math.min(idx, this.buffer.length - 1);
    if (clamped === this.currentIndex && this.prevFrame !== null) {
      // Same frame — still need to call progress for scrub UI responsiveness
      this.opts.onProgress(clamped, this.buffer.length);
      return;
    }
    this.currentIndex = clamped;
    // Sync playbackMs so scrub + speed changes stay consistent
    this.playbackMs = (clamped / TICK_RATE) * 1000;

    const frame = this.buffer[clamped];
    if (!frame) return;

    this.opts.onFrame(frame);
    this.prevFrame = frame;
    this.opts.onProgress(clamped, this.buffer.length);
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Derive the /observe WebSocket URL from the current page's location.
 * Falls back to ws://localhost:3000/observe in non-browser environments.
 */
function defaultObserveWsUrl(): string {
  if (typeof window === "undefined") return "ws://localhost:3000/observe";
  const win = window as typeof window & { ARENA_WS_URL?: string };
  if (win.ARENA_WS_URL) return win.ARENA_WS_URL;
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/observe`;
}
