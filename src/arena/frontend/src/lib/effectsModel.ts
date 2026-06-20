/**
 * effectsModel — pure model for transient visual effects (explosions, arc-lance beams, etc.).
 *
 * All functions are pure: they return new state without mutating inputs.
 * This makes the model trivially testable and reusable by the replay engine
 * and sound module.
 *
 * Transient effects are tracked by a numeric lifetime (ticksLeft).  Each call
 * to `advanceEffects` decrements all lifetimes and removes expired entries.
 *
 * Effects are now spawned directly from `frame.events` via `spawnEffectsFromEvents`.
 * No state-delta reconstruction is used anywhere in this module.
 */

import type { Vec2, GodEvent, GodShipView } from "./frameParser.ts";

// ── Constants ─────────────────────────────────────────────────────────────────

/** Default lifetime (in game ticks) for an explosion effect. */
export const EXPLOSION_LIFETIME_TICKS = 20;

/** Lifetime (in game ticks) for an Arc Lance beam effect — a brief flash. */
export const ARC_LANCE_LIFETIME_TICKS = 5;

// ── Types ─────────────────────────────────────────────────────────────────────

/** A single active transient visual effect. */
export type TransientEffect = ExplosionEffect | ArcLanceEffect;

/** Ship-destruction or mine-detonation explosion. */
export interface ExplosionEffect {
  kind: "explosion";
  /** Unique id. */
  id: string;
  /** World-space anchor position. */
  pos: Vec2;
  /** Remaining lifetime in ticks; 0 means expired (will be removed). */
  ticksLeft: number;
}

/**
 * Arc Lance beam — rendered as a line from attacker to target.
 * Resolves the issue-04 "Arc Lance unrenderable" limitation.
 */
export interface ArcLanceEffect {
  kind: "arcLance";
  /** Unique id. */
  id: string;
  /** Midpoint — used as spatial anchor for audio / glow. */
  pos: Vec2;
  /** Attacker (Arc Lance source) world position. */
  from: Vec2;
  /** Target (hit ship) world position. */
  to: Vec2;
  /** Remaining lifetime in ticks. */
  ticksLeft: number;
}

/** Complete immutable snapshot of all active transient effects. */
export interface EffectsState {
  readonly effects: readonly TransientEffect[];
  /** Monotonically increasing counter used to generate unique effect ids. */
  readonly nextId: number;
}

/** An empty effects state — use as the initial value. */
export const EMPTY_EFFECTS: EffectsState = { effects: [], nextId: 0 };

// ── Explosion input type (used by addExplosions) ──────────────────────────────

/** Minimal shape accepted by `addExplosions` — no dependency on deleted modules. */
type ExplosionInput = { shipId?: string; pos: Vec2 };

// ── Pure operations ───────────────────────────────────────────────────────────

/**
 * Spawn explosion effects from a list of explosion inputs, returning a new state.
 * Kept for direct use in tests and the effectsModel aging suite.
 *
 * @param state      Current effects state.
 * @param explosions Array of `{ shipId?, pos }` inputs.
 * @param lifetime   Ticks each explosion lives for (default: `EXPLOSION_LIFETIME_TICKS`).
 */
export function addExplosions(
  state: EffectsState,
  explosions: readonly ExplosionInput[],
  lifetime = EXPLOSION_LIFETIME_TICKS,
): EffectsState {
  if (explosions.length === 0) return state;

  let nextId = state.nextId;
  const newEffects: ExplosionEffect[] = explosions.map((ev) => ({
    kind: "explosion" as const,
    id: `explosion:${ev.shipId ?? "?"}:${nextId++}`,
    pos: ev.pos,
    ticksLeft: lifetime,
  }));

  return {
    effects: [...state.effects, ...newEffects],
    nextId,
  };
}

/**
 * Spawn transient effects from the authoritative `frame.events` array.
 *
 * Event → effect mapping:
 *   `died`          → `ExplosionEffect` at the subject ship's position
 *   `mineDetonated` → `ExplosionEffect` at the event's `pos` payload
 *   `lanceTookHull` → `ArcLanceEffect` from attacker (`by`) to target (`ship`)
 *                     (requires both ships present in the `ships` list)
 *
 * All other events produce no visual effect.
 *
 * @param state   Current effects state.
 * @param events  The `frame.events` array from the current god-view frame.
 * @param ships   The `frame.ships` array (used for position lookups).
 * @returns       New state with any newly-spawned effects appended.
 */
export function spawnEffectsFromEvents(
  state: EffectsState,
  events: readonly GodEvent[],
  ships: readonly GodShipView[],
): EffectsState {
  const shipById = new Map(ships.map((s) => [s.id, s]));
  let nextId = state.nextId;
  const newEffects: TransientEffect[] = [];

  for (const ev of events) {
    if (ev.event === "died") {
      const ship = shipById.get(ev.ship);
      const pos = ship?.pos ?? { x: 0, y: 0 };
      newEffects.push({
        kind: "explosion",
        id: `explosion:${ev.ship}:${nextId++}`,
        pos,
        ticksLeft: EXPLOSION_LIFETIME_TICKS,
      });
    } else if (ev.event === "mineDetonated") {
      newEffects.push({
        kind: "explosion",
        id: `mine:${ev.mineId}:${nextId++}`,
        pos: ev.pos,
        ticksLeft: EXPLOSION_LIFETIME_TICKS,
      });
    } else if (ev.event === "lanceTookHull") {
      const target = shipById.get(ev.ship);
      const attacker = shipById.get(ev.by);
      if (target && attacker) {
        const pos = {
          x: (target.pos.x + attacker.pos.x) / 2,
          y: (target.pos.y + attacker.pos.y) / 2,
        };
        newEffects.push({
          kind: "arcLance",
          id: `lance:${ev.ship}:${nextId++}`,
          pos,
          from: attacker.pos,
          to: target.pos,
          ticksLeft: ARC_LANCE_LIFETIME_TICKS,
        });
      }
    }
  }

  if (newEffects.length === 0) return state;
  return { effects: [...state.effects, ...newEffects], nextId };
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

  return { ...state, effects: surviving };
}
