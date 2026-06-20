/**
 * effectsModel — pure model for transient visual effects (explosions, etc.).
 *
 * All functions are pure: they return new state without mutating inputs.
 * This makes the model trivially testable and reusable by the replay engine
 * (issue 07) and sound module (issue 06).
 *
 * Transient effects are tracked by a numeric lifetime (ticksLeft).  Each call
 * to `advanceEffects` decrements all lifetimes and removes expired entries.
 *
 * Seam for future effect types: extend `TransientEffect["kind"]` with new
 * discriminants (e.g., "arcLance") when the server adds the relevant data.
 */

import type { Vec2 } from "./frameParser.ts";
import type { ExplosionEvent } from "./explosionDetector.ts";

// ── Constants ─────────────────────────────────────────────────────────────────

/** Default lifetime (in game ticks) for an explosion effect. */
export const EXPLOSION_LIFETIME_TICKS = 20;

// ── Types ─────────────────────────────────────────────────────────────────────

/** A single active transient visual effect. */
export interface TransientEffect {
  /** Unique id (e.g., "explosion:<shipId>:<tick>"). */
  id: string;
  /** World-space position where the effect is anchored. */
  pos: Vec2;
  /** Remaining lifetime in ticks; 0 means expired (will be removed). */
  ticksLeft: number;
  /** Discriminant — extendable for future effect types (e.g., arc-lance beam). */
  kind: "explosion";
}

/** Complete immutable snapshot of all active transient effects. */
export interface EffectsState {
  readonly effects: readonly TransientEffect[];
  /** Monotonically increasing counter used to generate unique effect ids. */
  readonly nextId: number;
}

/** An empty effects state — use as the initial value. */
export const EMPTY_EFFECTS: EffectsState = { effects: [], nextId: 0 };

// ── Pure operations ───────────────────────────────────────────────────────────

/**
 * Spawn explosion effects for each `ExplosionEvent`, returning a new state.
 *
 * @param state      Current effects state.
 * @param explosions Events returned by `detectExplosions`.
 * @param lifetime   Ticks each explosion lives for (default: `EXPLOSION_LIFETIME_TICKS`).
 */
export function addExplosions(
  state: EffectsState,
  explosions: readonly ExplosionEvent[],
  lifetime = EXPLOSION_LIFETIME_TICKS,
): EffectsState {
  if (explosions.length === 0) return state;

  let nextId = state.nextId;
  const newEffects: TransientEffect[] = explosions.map((ev) => ({
    id: `explosion:${ev.shipId}:${nextId++}`,
    pos: ev.pos,
    ticksLeft: lifetime,
    kind: "explosion",
  }));

  return {
    effects: [...state.effects, ...newEffects],
    nextId,
  };
}

/**
 * Advance the effects state by `ticks` game ticks, decrementing lifetimes and
 * removing expired effects.
 *
 * @param state  Current effects state.
 * @param ticks  Number of ticks to advance (typically 1 per frame).
 * @returns      New state with updated/removed effects.
 */
export function advanceEffects(
  state: EffectsState,
  ticks = 1,
): EffectsState {
  const surviving = state.effects
    .map((e) => ({ ...e, ticksLeft: e.ticksLeft - ticks }))
    .filter((e) => e.ticksLeft > 0);

  if (surviving.length === state.effects.length) {
    // Optimise: skip allocation if no effects expired
    // (length same implies same refs when no expiry — compare just length)
  }

  return { ...state, effects: surviving };
}
