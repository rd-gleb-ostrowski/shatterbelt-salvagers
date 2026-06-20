/**
 * soundCues — pure, framework-free event-driven → SoundCue[] mapping.
 *
 * The god-view frame's authoritative `events` array is the SOLE source of
 * truth for one-shot sound cues. Thrust is derived from the authoritative
 * ship STATE (per-ship `isThrusting` — this is live position data, not a
 * delta). Match lifecycle cues are derived from authoritative frame METADATA
 * (tick / maxTicks).
 *
 * NO state-delta reconstruction anywhere: this function takes ONE frame only.
 *
 * Pure function — no side effects, no global state; fully unit-testable.
 */

import type { GodViewFrame } from "./frameParser.ts";
import { isThrusting } from "./shipPresentation.ts";
import type { Vec2 } from "./frameParser.ts";

// ── SoundCue type ─────────────────────────────────────────────────────────────

/**
 * The kinds of sound events the Viewer can trigger.
 *
 * - explosion       — a ship was destroyed (from `died`) or mine detonated
 * - cannon          — a rune-cannon bolt was just fired (`cannonFired`)
 * - sigilDischarge  — a ship discharged its Sigil (`sigilDischarged`)
 * - relicPickup     — a ship collected a Relic (`relicTaken`)
 * - relicBank       — a ship banked Relics at its Anchor (`relicBanked`)
 * - lanceZap        — Arc Lance beam hit (`lanceTookHull`)
 * - thrust          — a ship is currently thrusting (drives continuous hum)
 * - matchStart      — tick === 0 (authoritative frame metadata)
 * - matchEnd        — tick >= maxTicks (authoritative frame metadata)
 */
export type SoundCueKind =
  | "explosion"
  | "cannon"
  | "sigilDischarge"
  | "relicPickup"
  | "relicBank"
  | "lanceZap"
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
 * Derive the set of sound cues that should play for the given god-view frame.
 *
 * Mapping rules:
 *
 * MATCH LIFECYCLE (authoritative frame metadata — not delta):
 *   matchStart  — `frame.tick === 0`
 *   matchEnd    — `frame.tick >= frame.maxTicks`
 *
 * EVENT-DRIVEN CUES (from `frame.events`):
 *   cannonFired       → cannon cue; pos from subject ship in frame.ships
 *   died              → explosion cue; pos from subject ship in frame.ships
 *   mineDetonated     → explosion cue; pos from event payload
 *   sigilDischarged   → sigilDischarge cue; sigil = ev.which
 *   relicTaken        → relicPickup cue
 *   relicBanked       → relicBank cue
 *   lanceTookHull     → lanceZap cue; pos from subject (hit) ship
 *
 * CONTINUOUS STATE (authoritative ship state — not a delta):
 *   thrust — `isThrusting(ship)` per alive ship each frame (drives hum loop)
 *
 * @param frame  Current god-view frame (single frame — no previous frame needed).
 * @returns      Array of `SoundCue`s (empty if nothing notable this tick).
 *
 * Pure function — deterministic, no I/O, no mutation of inputs.
 */
export function deriveSoundCues(frame: GodViewFrame): SoundCue[] {
  const cues: SoundCue[] = [];

  // ── Match lifecycle (authoritative frame metadata) ─────────────────────────

  if (frame.tick === 0) {
    cues.push({ kind: "matchStart" });
  }
  if (frame.tick >= frame.maxTicks) {
    cues.push({ kind: "matchEnd" });
  }

  // ── Event-driven one-shot cues ─────────────────────────────────────────────

  const shipById = new Map(frame.ships.map((s) => [s.id, s]));

  for (const ev of frame.events) {
    switch (ev.event) {
      case "cannonFired": {
        const ship = shipById.get(ev.ship);
        cues.push({ kind: "cannon", shipId: ev.ship, pos: ship?.pos });
        break;
      }
      case "died": {
        const ship = shipById.get(ev.ship);
        cues.push({ kind: "explosion", shipId: ev.ship, pos: ship?.pos });
        break;
      }
      case "mineDetonated": {
        cues.push({ kind: "explosion", pos: ev.pos });
        break;
      }
      case "sigilDischarged": {
        const ship = shipById.get(ev.ship);
        cues.push({
          kind: "sigilDischarge",
          shipId: ev.ship,
          pos: ship?.pos,
          sigil: ev.which,
        });
        break;
      }
      case "relicTaken": {
        const ship = shipById.get(ev.ship);
        cues.push({ kind: "relicPickup", shipId: ev.ship, pos: ship?.pos });
        break;
      }
      case "relicBanked": {
        const ship = shipById.get(ev.ship);
        cues.push({ kind: "relicBank", shipId: ev.ship, pos: ship?.pos });
        break;
      }
      case "lanceTookHull": {
        const ship = shipById.get(ev.ship);
        cues.push({ kind: "lanceZap", shipId: ev.ship, pos: ship?.pos });
        break;
      }
      default:
        break;
    }
  }

  // ── THRUST: per-alive-ship authoritative state (drives continuous hum) ──────

  for (const ship of frame.ships) {
    if (ship.alive && isThrusting(ship)) {
      cues.push({ kind: "thrust", shipId: ship.id, pos: ship.pos });
    }
  }

  return cues;
}
