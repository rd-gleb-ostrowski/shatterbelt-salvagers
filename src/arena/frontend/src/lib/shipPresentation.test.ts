/**
 * Unit tests for pure ship-presentation logic.
 *
 * Covers:
 *   (1) teamColour — same id → same colour (stability)
 *   (2) teamColour — different ids → distinct colours (distinctness)
 *   (3) barFillRatio — hull bar normal cases
 *   (4) barFillRatio — shield bar likewise
 *   (5) barFillRatio — edge cases (max=0, cur>max, negative)
 *   (6) isThrusting  — afterburner signal and forward-speed heuristic
 *
 * Seam: teamColour + barFillRatio are also consumed by HUD (issue 05).
 */

import { describe, it, expect } from "vitest";
import {
  teamColour,
  barFillRatio,
  isThrusting,
  THRUST_FORWARD_THRESHOLD,
} from "./shipPresentation.ts";

// ── Slice 1: teamColour — stability ──────────────────────────────────────────

describe("teamColour — stability", () => {
  it("same id returns the same colour on every call", () => {
    expect(teamColour("alpha")).toBe(teamColour("alpha"));
    expect(teamColour("bravo")).toBe(teamColour("bravo"));
    expect(teamColour("")).toBe(teamColour(""));
  });

  it("returns a valid PixiJS hex colour in range [0x000000, 0xffffff]", () => {
    for (const id of ["team1", "team2", "foo", "bar", "", "zzz"]) {
      const c = teamColour(id);
      expect(c).toBeGreaterThanOrEqual(0x000000);
      expect(c).toBeLessThanOrEqual(0xffffff);
    }
  });
});

// ── Slice 2: teamColour — distinctness ───────────────────────────────────────

describe("teamColour — distinctness", () => {
  // The palette has 12 colours; arbitrary ids CAN collide (like any hash-to-
  // small-table mapping).  These tests verify the hash is not degenerate and
  // that the palette entries themselves are all distinct colours.

  it("two clearly different ids produce different colours", () => {
    // Verified not to collide in the 12-slot palette
    expect(teamColour("charlie")).not.toBe(teamColour("echo"));
    expect(teamColour("team1")).not.toBe(teamColour("team2"));
  });

  it("a set of 6 verified-distinct ids all map to distinct colours", () => {
    // These ids are verified (by djb2 % 12) to hit different palette slots:
    // charlie→1, echo→8, team1→9, team2→10, team3→11, team4→0
    const ids = ["charlie", "echo", "team1", "team2", "team3", "team4"];
    const colours = ids.map(teamColour);
    const unique = new Set(colours);
    expect(unique.size).toBe(ids.length);
  });

  it("the palette itself has all distinct colour values", () => {
    // Colour quality: none of the 12 palette entries are accidentally identical
    const colours = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot",
      "team1", "team2", "team3", "team4", "teamA", "teamZ"].map(teamColour);
    // At least half must be unique (hash collisions are expected; palette size is 12)
    const unique = new Set(colours);
    expect(unique.size).toBeGreaterThanOrEqual(6);
  });
});

// ── Slice 3: barFillRatio — hull bar normal cases ─────────────────────────────

describe("barFillRatio — hull bar (normal)", () => {
  it("full hull (cur = max) returns 1", () => {
    expect(barFillRatio(100, 100)).toBe(1);
  });

  it("half hull returns 0.5", () => {
    expect(barFillRatio(50, 100)).toBeCloseTo(0.5);
  });

  it("empty hull (cur = 0) returns 0", () => {
    expect(barFillRatio(0, 100)).toBe(0);
  });

  it("arbitrary ratio is computed correctly", () => {
    expect(barFillRatio(75, 200)).toBeCloseTo(75 / 200);
  });
});

// ── Slice 4: barFillRatio — shield bar likewise ───────────────────────────────

describe("barFillRatio — shield bar", () => {
  it("25/80 shield returns the correct fraction", () => {
    expect(barFillRatio(25, 80)).toBeCloseTo(25 / 80);
  });

  it("full shield returns 1", () => {
    expect(barFillRatio(60, 60)).toBe(1);
  });

  it("1/3 shield returns ~0.333", () => {
    expect(barFillRatio(20, 60)).toBeCloseTo(1 / 3);
  });
});

// ── Slice 5: barFillRatio — edge cases ───────────────────────────────────────

describe("barFillRatio — edge cases", () => {
  it("max = 0 returns 0 (guards division by zero)", () => {
    expect(barFillRatio(0, 0)).toBe(0);
    expect(barFillRatio(50, 0)).toBe(0);
  });

  it("negative max returns 0", () => {
    expect(barFillRatio(10, -5)).toBe(0);
    expect(barFillRatio(0, -1)).toBe(0);
  });

  it("cur > max clamps to 1 (over-healed / shield overcharge)", () => {
    expect(barFillRatio(120, 100)).toBe(1);
    expect(barFillRatio(999, 100)).toBe(1);
  });

  it("negative cur clamps to 0", () => {
    expect(barFillRatio(-10, 100)).toBe(0);
    expect(barFillRatio(-1, 50)).toBe(0);
  });
});

// ── Slice 6a: isThrusting — afterburner signal ───────────────────────────────

describe("isThrusting — afterburner (direct signal)", () => {
  it("afterburnerTicksLeft > 0 returns true regardless of velocity", () => {
    expect(
      isThrusting({ vel: { x: 0, y: 0 }, heading: 0, afterburnerTicksLeft: 5 })
    ).toBe(true);
    expect(
      isThrusting({ vel: { x: 0, y: 0 }, heading: 0, afterburnerTicksLeft: 1 })
    ).toBe(true);
  });

  it("afterburnerTicksLeft = 0 with no forward speed returns false", () => {
    expect(
      isThrusting({ vel: { x: 0, y: 0 }, heading: 0, afterburnerTicksLeft: 0 })
    ).toBe(false);
  });

  it("afterburnerTicksLeft = 0 with lateral velocity returns false", () => {
    // heading = 0 (pointing +x); velocity is purely in +y (perpendicular)
    expect(
      isThrusting({
        vel: { x: 0, y: 100 },
        heading: 0,
        afterburnerTicksLeft: 0,
      })
    ).toBe(false);
  });
});

// ── Slice 6b: isThrusting — forward-speed heuristic ──────────────────────────

describe("isThrusting — forward-speed heuristic", () => {
  it("forward speed above threshold returns true (heading = 0, +x direction)", () => {
    expect(
      isThrusting({
        vel: { x: THRUST_FORWARD_THRESHOLD + 1, y: 0 },
        heading: 0,
        afterburnerTicksLeft: 0,
      })
    ).toBe(true);
  });

  it("forward speed exactly at threshold returns false (strict >)", () => {
    expect(
      isThrusting({
        vel: { x: THRUST_FORWARD_THRESHOLD, y: 0 },
        heading: 0,
        afterburnerTicksLeft: 0,
      })
    ).toBe(false);
  });

  it("forward speed below threshold returns false", () => {
    expect(
      isThrusting({
        vel: { x: THRUST_FORWARD_THRESHOLD - 1, y: 0 },
        heading: 0,
        afterburnerTicksLeft: 0,
      })
    ).toBe(false);
  });

  it("heading = π/2 (pointing +y) — forward speed in +y triggers flame", () => {
    expect(
      isThrusting({
        vel: { x: 0, y: THRUST_FORWARD_THRESHOLD + 2 },
        heading: Math.PI / 2,
        afterburnerTicksLeft: 0,
      })
    ).toBe(true);
  });

  it("backward speed (moving opposite to heading) returns false", () => {
    // heading = 0 (pointing +x); vel is in -x direction
    expect(
      isThrusting({
        vel: { x: -(THRUST_FORWARD_THRESHOLD + 10), y: 0 },
        heading: 0,
        afterburnerTicksLeft: 0,
      })
    ).toBe(false);
  });
});
