/**
 * hudFormatters — pure, framework-free HUD formatting functions.
 *
 * Exports:
 *   formatTimer(tick, maxTicks, tickRate?)  — remaining time as "m:ss"
 *   scoreboardRows(frame)                  — sorted scoreboard display rows
 *   ladderRows(standings)                  — formatted TrueSkill ladder rows
 *
 * All three are pure functions with no side effects, fully unit-testable.
 *
 * Seam for issue 07 (replay): formatTimer + scoreboardRows accept a
 * GodViewFrame and a tick value respectively, so the replay player can feed
 * recorded frames into the same formatters without modification.
 */

import type { GodViewFrame } from "./frameParser.ts";
import { teamColour } from "./shipPresentation.ts";

// ── Timer ─────────────────────────────────────────────────────────────────────

/**
 * Convert a current tick position into a remaining-time string "m:ss".
 *
 * Algorithm:
 *   remainingTicks = clamp(maxTicks − tick, 0, ∞)
 *   totalSeconds   = floor(remainingTicks / tickRate)
 *   format as      "{minutes}:{paddedSeconds}"
 *
 * @param tick      Current simulation tick (0-based).
 * @param maxTicks  Total ticks in the match (default arena value: 3600).
 * @param tickRate  Engine tick rate in ticks/second (default: 30).
 * @returns         Remaining time formatted as "m:ss", e.g. "2:00" or "0:05".
 */
export function formatTimer(
  tick: number,
  maxTicks: number,
  tickRate = 30,
): string {
  const remainingTicks = Math.max(0, maxTicks - tick);
  const totalSeconds = Math.floor(remainingTicks / tickRate);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

// ── Scoreboard ────────────────────────────────────────────────────────────────

/** One row in the live scoreboard, ready for rendering. */
export interface ScoreboardRow {
  /** Team/competitor identifier (matches scores keys and ship ids). */
  team: string;
  /** Banked score for this team. */
  score: number;
  /** Total relics currently carried by all ships belonging to this team. */
  relicsCarried: number;
  /** PixiJS-compatible 0xRRGGBB colour assigned to this team. */
  colour: number;
}

/**
 * Derive the scoreboard display rows from a live god-view frame.
 *
 * Each entry in `frame.scores` becomes a row. The ship id IS the team
 * identity (one competitor = one ship = one scores key). relicsCarried is
 * summed across all ships whose id matches the team key, so the function
 * remains correct even if the engine ever allows multiple ships per team.
 *
 * Rows are sorted by score descending (highest score first). Ties preserve
 * the order returned by `Object.entries` (insertion order).
 *
 * Pure function — deterministic, no side effects.
 */
export function scoreboardRows(frame: GodViewFrame): ScoreboardRow[] {
  const rows: ScoreboardRow[] = Object.entries(frame.scores).map(
    ([team, score]) => {
      const relicsCarried = frame.ships
        .filter((s) => s.id === team)
        .reduce((sum, s) => sum + s.relicsCarried, 0);
      return { team, score, relicsCarried, colour: teamColour(team) };
    },
  );
  rows.sort((a, b) => b.score - a.score);
  return rows;
}

// ── Ladder ────────────────────────────────────────────────────────────────────

/** Raw standing as returned by GET /ladder/standings. */
export interface LadderStanding {
  competitor: string;
  mu: number;
  sigma: number;
  conservativeSkill: number;
  matches: number;
}

/** One row in the TrueSkill ladder display panel. */
export interface LadderRow {
  /** Competitor name. */
  competitor: string;
  /** conservativeSkill formatted to one decimal place (e.g. "23.4"). */
  conservativeSkill: string;
  /** Number of rated matches played. */
  matches: number;
}

/**
 * Format raw standings into display rows, sorted by conservativeSkill
 * descending (highest skill first).
 *
 * The server already returns standings in this order, but the formatter
 * re-sorts for robustness so callers can pass unsorted or partially-cached
 * data and still get the correct display order.
 *
 * conservativeSkill is formatted with one decimal place to keep the column
 * narrow while preserving meaningful precision.
 *
 * Pure function — deterministic, no side effects.
 */
export function ladderRows(standings: LadderStanding[]): LadderRow[] {
  const sorted = [...standings].sort(
    (a, b) => b.conservativeSkill - a.conservativeSkill,
  );
  return sorted.map((s) => ({
    competitor: s.competitor,
    conservativeSkill: s.conservativeSkill.toFixed(1),
    matches: s.matches,
  }));
}
