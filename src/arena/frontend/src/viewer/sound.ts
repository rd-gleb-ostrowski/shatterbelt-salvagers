/**
 * sound — Web Audio synthesis layer for the Viewer.
 *
 * Receives `SoundCue[]` from `deriveSoundCues` each tick and plays synthesised
 * sounds via the browser-native Web Audio API. No external audio files needed.
 *
 * DESIGN NOTES
 * ─────────────
 * The server's god-view stream contains NO `events` array; all cues are
 * delta-derived by the pure `deriveSoundCues` function. This module is the
 * boundary between that pure logic and stateful browser audio playback.
 * If the server ever adds an events channel the mapping function can be
 * updated independently of this file.
 *
 * USER-GESTURE UNLOCK
 *   Browsers suspend the AudioContext until the user interacts with the page.
 *   `SoundEngine.unlock()` resumes the context on the first user gesture; call
 *   it in a click/keydown handler. Sounds queued before unlock are silently
 *   dropped (the match opening is more important than the stinger).
 *
 * THRUST HUM
 *   Thrust is continuous — a ship thrusting for many consecutive ticks should
 *   produce a hum, not a rapid sequence of pops. The engine maintains a map of
 *   active hum oscillators keyed by shipId. On each tick it starts an oscillator
 *   for ships that are now thrusting and stops the one for ships that stopped.
 *
 * SPATIAL PANNING
 *   `SoundCue.pos` is used to pan stereo: a world X position is mapped to a
 *   [-1, 1] stereo pan based on the arena width supplied at construction.
 *   Explosions and cannon fire are panned; match stingers play centred.
 *
 * NOT UNIT-TESTED (per PRD): this module creates Web Audio nodes which are
 * browser-only. Rely on manual/audible verification in a live match.
 *
 * Seam for issue 07 (replay): replay calls `engine.playCues(deriveSoundCues(frame))`
 * on each step at whatever playback speed — the same interface as the live path.
 */

import type { SoundCue } from "../lib/soundCues.ts";

// ── Synth helpers ─────────────────────────────────────────────────────────────

/**
 * Create a StereoPannerNode (or a no-op pass-through if unsupported) given a
 * pan value in [-1, 1].
 */
function makePanner(ctx: AudioContext, pan: number): AudioNode {
  if (typeof StereoPannerNode !== "undefined") {
    const node = ctx.createStereoPanner();
    node.pan.value = Math.max(-1, Math.min(1, pan));
    return node;
  }
  // Fallback: connect straight to destination
  return ctx.destination;
}

/**
 * Route `source → panner → destination` (or `source → destination` if panner
 * IS the destination).
 */
function connect(source: AudioNode, panner: AudioNode, ctx: AudioContext): void {
  source.connect(panner);
  if (panner !== ctx.destination) {
    panner.connect(ctx.destination);
  }
}

// ── SoundEngine ───────────────────────────────────────────────────────────────

export class SoundEngine {
  private ctx: AudioContext | null = null;
  /** arenaWidth used to map world-X → stereo pan. */
  private readonly arenaWidth: number;
  /** Active thrust-hum oscillators keyed by shipId. */
  private readonly thrustNodes = new Map<string, OscillatorNode>();

  constructor(arenaWidth = 1000) {
    this.arenaWidth = arenaWidth;
  }

  // ── Public API ──────────────────────────────────────────────────────────────

  /**
   * Resume the AudioContext. Must be called from a user-gesture handler
   * (click, keydown, etc.) to satisfy the browser's autoplay policy.
   */
  unlock(): void {
    if (!this.ctx) {
      this.ctx = new AudioContext();
    }
    if (this.ctx.state === "suspended") {
      this.ctx.resume().catch(() => undefined);
    }
  }

  /**
   * Process a batch of cues for this tick. Call once per rendered frame with
   * the output of `deriveSoundCues(frame)`.
   *
   * Silently no-ops if the AudioContext has not been unlocked yet.
   */
  playCues(cues: readonly SoundCue[]): void {
    if (!this.ctx || this.ctx.state !== "running") return;

    // Collect the set of thrusting ship ids this tick
    const thrustingNow = new Set<string>();

    for (const cue of cues) {
      switch (cue.kind) {
        case "explosion":
          this.playExplosion(cue.pos?.x);
          break;
        case "cannon":
          this.playCannon(cue.pos?.x);
          break;
        case "sigilDischarge":
          this.playSigilDischarge(cue.sigil, cue.pos?.x);
          break;
        case "relicPickup":
          this.playRelicPickup(cue.pos?.x);
          break;
        case "relicBank":
          this.playRelicBank(cue.pos?.x);
          break;
        case "lanceZap":
          this.playLanceZap(cue.pos?.x);
          break;
        case "thrust":
          if (cue.shipId) thrustingNow.add(cue.shipId);
          break;
        case "matchStart":
          this.playMatchStart();
          break;
        case "matchEnd":
          this.playMatchEnd();
          break;
      }
    }

    // Reconcile thrust hum oscillators
    this.reconcileThrust(thrustingNow);
  }

  /** Stop all sound and release the AudioContext. */
  dispose(): void {
    for (const osc of this.thrustNodes.values()) {
      try { osc.stop(); } catch { /* already stopped */ }
    }
    this.thrustNodes.clear();
    if (this.ctx) {
      this.ctx.close().catch(() => undefined);
      this.ctx = null;
    }
  }

  // ── Private synth implementations ──────────────────────────────────────────

  private pan(worldX: number | undefined): number {
    if (worldX === undefined) return 0;
    // Map [0, arenaWidth] → [-0.8, 0.8] (leave headroom at edges)
    return ((worldX / this.arenaWidth) * 2 - 1) * 0.8;
  }

  /** Low-pitched burst + noise swoosh for ship destruction. */
  private playExplosion(worldX?: number): void {
    const ctx = this.ctx!;
    const t = ctx.currentTime;
    const panner = makePanner(ctx, this.pan(worldX));

    // Sub-bass thud
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sawtooth";
    osc.frequency.setValueAtTime(80, t);
    osc.frequency.exponentialRampToValueAtTime(20, t + 0.4);
    gain.gain.setValueAtTime(0.6, t);
    gain.gain.exponentialRampToValueAtTime(0.001, t + 0.5);
    osc.connect(gain);
    connect(gain, panner, ctx);
    osc.start(t);
    osc.stop(t + 0.5);

    // Noise burst
    const bufSize = ctx.sampleRate * 0.3;
    const buf = ctx.createBuffer(1, bufSize, ctx.sampleRate);
    const data = buf.getChannelData(0);
    for (let i = 0; i < bufSize; i++) data[i] = Math.random() * 2 - 1;
    const noise = ctx.createBufferSource();
    noise.buffer = buf;
    const noiseGain = ctx.createGain();
    noiseGain.gain.setValueAtTime(0.4, t);
    noiseGain.gain.exponentialRampToValueAtTime(0.001, t + 0.3);
    noise.connect(noiseGain);
    connect(noiseGain, panner, ctx);
    noise.start(t);
  }

  /** Sharp arcane crack for rune-cannon bolt. */
  private playCannon(worldX?: number): void {
    const ctx = this.ctx!;
    const t = ctx.currentTime;
    const panner = makePanner(ctx, this.pan(worldX));

    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "square";
    osc.frequency.setValueAtTime(600, t);
    osc.frequency.exponentialRampToValueAtTime(150, t + 0.08);
    gain.gain.setValueAtTime(0.35, t);
    gain.gain.exponentialRampToValueAtTime(0.001, t + 0.1);
    osc.connect(gain);
    connect(gain, panner, ctx);
    osc.start(t);
    osc.stop(t + 0.1);
  }

  /**
   * Sigil discharge — each Sigil gets a distinct timbre:
   *   Afterburner  — rising whoosh
   *   Bulwark      — bright shield chord
   *   Singularity  — deep spiral drone
   *   AetherMine   — metallic click
   *   ArcLance     — piercing sizzle
   *   (fallback)   — generic bell tone
   */
  private playSigilDischarge(sigil: string | undefined, worldX?: number): void {
    const ctx = this.ctx!;
    const t = ctx.currentTime;
    const panner = makePanner(ctx, this.pan(worldX));

    const osc = ctx.createOscillator();
    const gain = ctx.createGain();

    switch (sigil) {
      case "Afterburner":
        osc.type = "sawtooth";
        osc.frequency.setValueAtTime(200, t);
        osc.frequency.exponentialRampToValueAtTime(800, t + 0.25);
        gain.gain.setValueAtTime(0.4, t);
        gain.gain.exponentialRampToValueAtTime(0.001, t + 0.3);
        break;
      case "Bulwark":
        osc.type = "triangle";
        osc.frequency.setValueAtTime(440, t);
        osc.frequency.setValueAtTime(660, t + 0.05);
        osc.frequency.setValueAtTime(880, t + 0.1);
        gain.gain.setValueAtTime(0.35, t);
        gain.gain.exponentialRampToValueAtTime(0.001, t + 0.4);
        break;
      case "Singularity":
        osc.type = "sine";
        osc.frequency.setValueAtTime(120, t);
        osc.frequency.exponentialRampToValueAtTime(30, t + 0.6);
        gain.gain.setValueAtTime(0.5, t);
        gain.gain.exponentialRampToValueAtTime(0.001, t + 0.7);
        break;
      case "AetherMine":
        osc.type = "square";
        osc.frequency.setValueAtTime(1200, t);
        osc.frequency.exponentialRampToValueAtTime(400, t + 0.05);
        gain.gain.setValueAtTime(0.3, t);
        gain.gain.exponentialRampToValueAtTime(0.001, t + 0.06);
        break;
      case "ArcLance":
        osc.type = "sawtooth";
        osc.frequency.setValueAtTime(1800, t);
        osc.frequency.exponentialRampToValueAtTime(900, t + 0.15);
        gain.gain.setValueAtTime(0.4, t);
        gain.gain.exponentialRampToValueAtTime(0.001, t + 0.2);
        break;
      default:
        // Generic bell
        osc.type = "sine";
        osc.frequency.setValueAtTime(880, t);
        osc.frequency.exponentialRampToValueAtTime(440, t + 0.2);
        gain.gain.setValueAtTime(0.3, t);
        gain.gain.exponentialRampToValueAtTime(0.001, t + 0.3);
    }

    osc.connect(gain);
    connect(gain, panner, ctx);
    osc.start(t);
    osc.stop(t + 0.8);
  }

  /** Bright chime on Relic pickup. */
  private playRelicPickup(worldX?: number): void {
    const ctx = this.ctx!;
    const t = ctx.currentTime;
    const panner = makePanner(ctx, this.pan(worldX));

    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sine";
    osc.frequency.setValueAtTime(1047, t); // C6
    osc.frequency.setValueAtTime(1319, t + 0.08); // E6
    gain.gain.setValueAtTime(0.3, t);
    gain.gain.exponentialRampToValueAtTime(0.001, t + 0.25);
    osc.connect(gain);
    connect(gain, panner, ctx);
    osc.start(t);
    osc.stop(t + 0.25);
  }

  /** Ascending arpeggio chime on Relic bank (score). */
  private playRelicBank(worldX?: number): void {    const ctx = this.ctx!;
    const panner = makePanner(ctx, this.pan(worldX));

    // Three-note ascending arpeggio
    const notes = [523, 659, 784]; // C5, E5, G5
    notes.forEach((freq, i) => {
      const t = ctx.currentTime + i * 0.07;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.setValueAtTime(freq, t);
      gain.gain.setValueAtTime(0.35, t);
      gain.gain.exponentialRampToValueAtTime(0.001, t + 0.2);
      osc.connect(gain);
      connect(gain, panner, ctx);
      osc.start(t);
      osc.stop(t + 0.2);
    });
  }

  /** Sharp electrical sizzle for Arc Lance beam hit. */
  private playLanceZap(worldX?: number): void {
    const ctx = this.ctx!;
    const t = ctx.currentTime;
    const panner = makePanner(ctx, this.pan(worldX));

    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sawtooth";
    osc.frequency.setValueAtTime(2200, t);
    osc.frequency.exponentialRampToValueAtTime(600, t + 0.06);
    gain.gain.setValueAtTime(0.3, t);
    gain.gain.exponentialRampToValueAtTime(0.001, t + 0.08);
    osc.connect(gain);
    connect(gain, panner, ctx);
    osc.start(t);
    osc.stop(t + 0.1);
  }

  /**
   * Manage thrust-hum oscillators.
   *
   * Start a new oscillator for each newly-thrusting ship; stop the oscillator
   * for each ship that was thrusting but no longer is. The oscillator pitch is
   * slightly randomised per shipId so overlapping ships don't phase-cancel.
   */
  private reconcileThrust(thrustingNow: ReadonlySet<string>): void {
    const ctx = this.ctx!;

    // Stop ships that stopped thrusting
    for (const [id, osc] of this.thrustNodes) {
      if (!thrustingNow.has(id)) {
        try { osc.stop(); } catch { /* already stopped */ }
        this.thrustNodes.delete(id);
      }
    }

    // Start oscillators for newly thrusting ships
    for (const id of thrustingNow) {
      if (this.thrustNodes.has(id)) continue; // already humming

      // Small per-ship pitch variation so multiple hums don't cancel
      const jitter = (hashStr(id) % 20) - 10; // ±10 Hz
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sawtooth";
      osc.frequency.value = 90 + jitter;
      gain.gain.value = 0.06; // quiet background hum
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start();
      this.thrustNodes.set(id, osc);
    }
  }

  /** Two-note ascending match-start fanfare. */
  private playMatchStart(): void {
    const ctx = this.ctx!;
    const notes = [392, 523, 659, 784]; // G4 C5 E5 G5
    notes.forEach((freq, i) => {
      const t = ctx.currentTime + i * 0.12;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "triangle";
      osc.frequency.setValueAtTime(freq, t);
      gain.gain.setValueAtTime(0.4, t);
      gain.gain.exponentialRampToValueAtTime(0.001, t + 0.3);
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start(t);
      osc.stop(t + 0.35);
    });
  }

  /** Descending match-end stinger. */
  private playMatchEnd(): void {
    const ctx = this.ctx!;
    const notes = [784, 659, 523, 392]; // G5 E5 C5 G4
    notes.forEach((freq, i) => {
      const t = ctx.currentTime + i * 0.15;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "triangle";
      osc.frequency.setValueAtTime(freq, t);
      gain.gain.setValueAtTime(0.4, t);
      gain.gain.exponentialRampToValueAtTime(0.001, t + 0.4);
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start(t);
      osc.stop(t + 0.45);
    });
  }
}

// ── Tiny string hash for per-ship pitch jitter ────────────────────────────────

function hashStr(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}
