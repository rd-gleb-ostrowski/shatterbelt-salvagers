/**
 * explosionDetector — pure, framework-free frame-delta logic for ship deaths.
 *
 * Given the previous frame's ship list and the current frame's ship list,
 * detects which ships transitioned from alive → dead/absent in this tick.
 * Each such ship produces an `ExplosionEvent` at its last known position.
 *
 * Rules:
 *   - alive last frame, not-alive (alive=false) this frame  → explosion
 *   - alive last frame, absent from this frame              → explosion
 *   - stays alive                                           → no explosion
 *   - stays dead across frames                              → no new explosion
 *   - dead last frame, alive this frame (respawn)           → no explosion
 *
 * Pure function — no side effects, no global state; fully unit-testable.
 *
 * Seam for issue 06 (sound): pass the same `ExplosionEvent[]` to the sound
 * module to trigger explosion SFX without duplicating delta detection.
 * Seam for issue 07 (replay): replay drives `renderFrame` with recorded frames;
 * the same call chain naturally feeds into this detector on every frame step.
 */

import type { GodShipView, Vec2 } from "./frameParser.ts";

// ── Types ─────────────────────────────────────────────────────────────────────

/**
 * A ship-destruction event detected from a frame-to-frame delta.
 * The position is the ship's last known world position (from the frame where
 * it was last alive), suitable for spawning an explosion effect.
 */
export interface ExplosionEvent {
  /** The destroyed ship's id. */
  shipId: string;
  /** Last known world position — spawn the explosion here. */
  pos: Vec2;
}

// ── detectExplosions ──────────────────────────────────────────────────────────

/**
 * Compute which ships were destroyed between two consecutive frames.
 *
 * @param prevShips  Ships array from the previous god-view frame.
 * @param currShips  Ships array from the current god-view frame.
 * @returns          Array of `ExplosionEvent`s (empty if none destroyed).
 *
 * Pure function — deterministic, no I/O, no mutation of inputs.
 */
export function detectExplosions(
  prevShips: readonly GodShipView[],
  currShips: readonly GodShipView[],
): ExplosionEvent[] {
  // Build a lookup map for the current frame to avoid O(n²)
  const currById = new Map<string, GodShipView>();
  for (const ship of currShips) {
    currById.set(ship.id, ship);
  }

  const events: ExplosionEvent[] = [];

  for (const prev of prevShips) {
    if (!prev.alive) continue; // ship was already dead — no new explosion

    const curr = currById.get(prev.id);
    if (curr === undefined) {
      // Ship absent from current frame: treat as destroyed
      events.push({ shipId: prev.id, pos: prev.pos });
    } else if (!curr.alive) {
      // Ship present but dead: use its current position (closest to death point)
      events.push({ shipId: prev.id, pos: curr.pos });
    }
    // else: ship still alive — no explosion
  }

  return events;
}
