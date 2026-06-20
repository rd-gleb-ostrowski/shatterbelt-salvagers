/**
 * soundCues — pure, framework-free frame-delta → SoundCue[] mapping.
 *
 * REALITY NOTE: The server's god-view stream (GodViewFrameJson in observer.rs)
 * does NOT contain an `events` array — the same gap noted in issues 01 and 04.
 * All sound cues are therefore DERIVED from frame-to-frame state deltas. This
 * is a pure, fully unit-testable function: given two consecutive frames it
 * emits the sound cues that should play this tick.
 *
 * Seam for issue 07 (replay): replay drives `deriveSoundCues` from recorded
 * frames on every step — the same function, no changes required.
 *
 * Seam for a future server events channel: if the server ever adds an `events`
 * array to the god-view frame, this module can be replaced or augmented
 * without touching the audio layer.
 *
 * Pure function — no side effects, no global state; fully unit-testable.
 */

import type { GodViewFrame } from "./frameParser.ts";
import { detectExplosions } from "./explosionDetector.ts";
import { isThrusting } from "./shipPresentation.ts";
import type { Vec2 } from "./frameParser.ts";

// ── SoundCue type ─────────────────────────────────────────────────────────────

/**
 * The kinds of sound events the Viewer can trigger.
 *
 * - explosion       — a ship was destroyed this tick
 * - cannon          — a rune-cannon bolt was just fired
 * - sigilDischarge  — a ship discharged its Sigil (spent the ability)
 * - relicPickup     — a ship collected a Relic
 * - relicBank       — a ship banked Relics at its Anchor (scoring)
 * - thrust          — a ship is currently thrusting (drives continuous hum)
 * - matchStart      — the match just started (tick 0 first seen)
 * - matchEnd        — the match just ended (tick reached maxTicks)
 */
export type SoundCueKind =
  | "explosion"
  | "cannon"
  | "sigilDischarge"
  | "relicPickup"
  | "relicBank"
  | "thrust"
  | "matchStart"
  | "matchEnd";

/**
 * A single sound cue to be played this tick.
 *
 * Optional fields let the audio layer spatialise or vary the sound:
 *   - `shipId`  — which ship triggered the cue (for panning / pitch variation)
 *   - `pos`     — world position for spatial audio
 *   - `sigil`   — Sigil type string for `sigilDischarge` (e.g. "Afterburner")
 */
export interface SoundCue {
  kind: SoundCueKind;
  shipId?: string;
  pos?: Vec2;
  sigil?: string;
}

// ── deriveSoundCues ───────────────────────────────────────────────────────────

/**
 * Derive the set of sound cues that should play when the viewer advances from
 * `prevFrame` to `currFrame`.
 *
 * Pass `null` for `prevFrame` on the very first frame received.
 *
 * Delta rules documented inline:
 *
 * EXPLOSION
 *   Reuses `detectExplosions(prev.ships, curr.ships)` — one cue per ship that
 *   transitioned alive→dead or alive→absent.
 *
 * CANNON FIRE
 *   A ship's `cannonCooldown` went from exactly 0 in `prev` to > 0 in `curr`
 *   while the ship is still alive in `curr`. The cooldown starts at 0 when the
 *   rune-cannon is ready; firing sets it to the reload duration and it counts
 *   down. The 0→positive transition is the only reliable "just fired" signal.
 *
 * SIGIL DISCHARGE
 *   A ship's `sigil` was non-null in `prev` and is null in `curr`, AND the ship
 *   is still alive in `curr`. A ship that died while holding a Sigil drops it
 *   silently (no discharge cue).
 *
 * RELIC PICKUP
 *   A ship's `relicsCarried` increased between frames.
 *
 * RELIC BANK
 *   A ship's `relicsCarried` decreased (or reached 0) AND the ship's score in
 *   `scores` increased in the same tick. The dual condition distinguishes a
 *   deliberate bank at the Anchor from relics dropped on death.
 *
 * THRUST
 *   Reuses `isThrusting(ship)` from shipPresentation for each alive ship in
 *   `currFrame`. Emitting a cue every tick lets the audio layer manage a
 *   continuous hum loop (start on first cue, silence when absent).
 *
 * MATCH START
 *   `curr.tick === 0` AND (`prevFrame` is null OR `prev.tick !== 0`). Fires
 *   exactly once at the beginning of each match even if tick-0 frames repeat.
 *
 * MATCH END
 *   `curr.tick >= curr.maxTicks` AND (`prevFrame` is null OR
 *   `prev.tick < curr.maxTicks`). Fires exactly once when the match crosses the
 *   final tick, preventing repeat stingers if extra frames arrive at maxTicks.
 *
 * @param prevFrame  Previous god-view frame, or `null` for the first frame.
 * @param currFrame  Current god-view frame.
 * @returns          Array of `SoundCue`s (empty if nothing notable happened).
 *
 * Pure function — deterministic, no I/O, no mutation of inputs.
 */
export function deriveSoundCues(
  prevFrame: GodViewFrame | null,
  currFrame: GodViewFrame,
): SoundCue[] {
  const cues: SoundCue[] = [];

  // ── Match lifecycle stingers ──────────────────────────────────────────────

  // MATCH START: first time we observe tick 0 in this match
  if (
    currFrame.tick === 0 &&
    (prevFrame === null || prevFrame.tick !== 0)
  ) {
    cues.push({ kind: "matchStart" });
  }

  // MATCH END: tick just crossed maxTicks
  if (
    currFrame.tick >= currFrame.maxTicks &&
    (prevFrame === null || prevFrame.tick < currFrame.maxTicks)
  ) {
    cues.push({ kind: "matchEnd" });
  }

  // ── Per-ship deltas (require prevFrame) ──────────────────────────────────

  if (prevFrame !== null) {
    // Build lookup maps for efficient O(1) access
    const prevShipById = new Map(prevFrame.ships.map((s) => [s.id, s]));
    const prevScores = prevFrame.scores;
    const currScores = currFrame.scores;

    // EXPLOSIONS: reuse detectExplosions for the alive→dead/absent delta
    const explosions = detectExplosions(prevFrame.ships, currFrame.ships);
    for (const e of explosions) {
      cues.push({ kind: "explosion", shipId: e.shipId, pos: e.pos });
    }

    for (const curr of currFrame.ships) {
      const prev = prevShipById.get(curr.id);
      if (prev === undefined) continue; // new ship — no prev state to diff

      // CANNON FIRE: cannonCooldown was 0 (ready), now > 0 (just fired).
      // Only on ships alive in curr so we don't double-up with explosion.
      if (curr.alive && prev.cannonCooldown === 0 && curr.cannonCooldown > 0) {
        cues.push({ kind: "cannon", shipId: curr.id, pos: curr.pos });
      }

      // SIGIL DISCHARGE: sigil present in prev, absent in curr, ship alive.
      if (curr.alive && prev.sigil !== null && curr.sigil === null) {
        cues.push({
          kind: "sigilDischarge",
          shipId: curr.id,
          pos: curr.pos,
          sigil: prev.sigil,
        });
      }

      // RELIC PICKUP: relicsCarried increased
      if (curr.relicsCarried > prev.relicsCarried) {
        cues.push({ kind: "relicPickup", shipId: curr.id, pos: curr.pos });
      }

      // RELIC BANK: relicsCarried decreased AND score increased.
      // Distinguishes deliberate banking from relics dropped on death.
      if (
        curr.relicsCarried < prev.relicsCarried &&
        (currScores[curr.id] ?? 0) > (prevScores[curr.id] ?? 0)
      ) {
        cues.push({ kind: "relicBank", shipId: curr.id, pos: curr.pos });
      }
    }
  }

  // ── THRUST: per-alive-ship in currFrame (works with or without prevFrame) ─

  for (const ship of currFrame.ships) {
    if (ship.alive && isThrusting(ship)) {
      cues.push({ kind: "thrust", shipId: ship.id, pos: ship.pos });
    }
  }

  return cues;
}
