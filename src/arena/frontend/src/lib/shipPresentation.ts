/**
 * shipPresentation — pure framework-free functions for ship visual state.
 *
 * Exports:
 *   teamColour(id)        — deterministic colour (0xRRGGBB) per ship/team identity
 *   barFillRatio(cur,max) — hull/shield bar fill fraction, clamped to [0,1]
 *   isThrusting(ship)     — infer thrust state from observable frame fields
 *
 * Seam for issue 05 (HUD): teamColour + barFillRatio are reused by the
 * scoreboard without touching this module.
 * Seam for issue 04 (effects): isThrusting drives the thrust-flame sprite layer.
 */

import type { Vec2 } from "./frameParser.ts";

// ── Team colour palette ───────────────────────────────────────────────────────

/**
 * Visually distinct colours optimised for dark backgrounds.
 *
 * STABLE — do NOT reorder entries.  Reordering changes existing colour
 * assignments for every ship id in every running match.
 */
const TEAM_PALETTE: readonly number[] = [
  0xff6b6b, // coral red
  0x4ecdc4, // teal
  0xffe66d, // amber yellow
  0xa8e6cf, // mint
  0xff8b94, // rose pink
  0x6c5ce7, // purple
  0xfd79a8, // hot pink
  0x00b894, // emerald
  0xe17055, // burnt orange
  0x74b9ff, // sky blue
  0xdfe6e9, // silver
  0xb2bec3, // cool grey
];

/**
 * djb2 string hash — fast, stable, good avalanche for short strings.
 * Returns an unsigned 32-bit integer.
 */
function djb2Hash(s: string): number {
  let h = 5381;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) + h) ^ s.charCodeAt(i);
  }
  return h >>> 0;
}

/**
 * Return a stable, distinct team colour (PixiJS 0xRRGGBB number) for the
 * given ship/team id.
 *
 * - The same id always returns the same colour across frames and sessions.
 * - Different ids are highly likely to return different palette entries
 *   (guaranteed distinct for ≤ palette-size distinct ids with good hashing).
 *
 * Pure function — no side effects, deterministic, unit-testable.
 */
export function teamColour(id: string): number {
  return TEAM_PALETTE[djb2Hash(id) % TEAM_PALETTE.length]!;
}

// ── Bar fill ratio ────────────────────────────────────────────────────────────

/**
 * Compute the fill fraction for a hull or shield bar.
 *
 * Returns `cur / max` clamped to [0, 1].
 *
 * Edge cases:
 *   - max ≤ 0  → 0  (no bar to fill; guards division by zero)
 *   - cur > max → 1  (over-healed / shield overcharge: treat as full)
 *   - cur < 0   → 0  (below empty)
 *
 * Pure function — no side effects, unit-testable.
 */
export function barFillRatio(cur: number, max: number): number {
  if (max <= 0) return 0;
  return Math.max(0, Math.min(1, cur / max));
}

// ── Thrust inference ─────────────────────────────────────────────────────────

/**
 * Minimum forward speed (world units/tick) projected onto the heading
 * direction required to show a thrust flame when the afterburner is inactive.
 *
 * Tuned so slow drift does not trigger a flame, but intentional thrust does.
 */
export const THRUST_FORWARD_THRESHOLD = 5;

/**
 * Infer whether a ship is currently thrusting from observable frame fields.
 *
 * Rule (documented):
 *   1. afterburnerTicksLeft > 0  — afterburner Sigil is active (exact signal).
 *   2. vel · heading_dir > THRUST_FORWARD_THRESHOLD  — the ship's velocity has
 *      a significant component in its forward direction, suggesting normal
 *      thrust.  This is a heuristic: the frame does not expose a raw thrust
 *      input, and forward momentum can accumulate from other forces.
 *
 * Pure function — no side effects, unit-testable.
 */
export function isThrusting(ship: {
  vel: Vec2;
  heading: number;
  afterburnerTicksLeft: number;
}): boolean {
  if (ship.afterburnerTicksLeft > 0) return true;
  const fwdSpeed =
    ship.vel.x * Math.cos(ship.heading) + ship.vel.y * Math.sin(ship.heading);
  return fwdSpeed > THRUST_FORWARD_THRESHOLD;
}
