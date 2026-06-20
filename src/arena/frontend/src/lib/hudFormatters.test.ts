/**
 * Unit tests for pure HUD formatting logic.
 *
 * Covers:
 *   Slice 1  — timer at tick 0 of 3600 → "2:00"
 *   Slice 2  — timer mid-match → correct remaining string
 *   Slice 3  — timer at/over maxTicks → "0:00"
 *   Slice 4  — timer seconds formatting with leading zero (e.g. "1:05")
 *   Slice 5  — scoreboard rows sorted by score desc, team colour, summed relics
 *   Slice 6  — scoreboard tie-breaking / empty frame handled gracefully
 *   Slice 7  — ladder rows formatted and ordered by conservativeSkill desc
 *
 * DOM/PixiJS overlay, fetch() wiring, and visibility logic are manual/visual
 * only (see PRD Testing Decisions).
 */

import { describe, it, expect } from "vitest";
import {
  formatTimer,
  scoreboardRows,
  ladderRows,
} from "./hudFormatters.ts";
import type { LadderStanding } from "./hudFormatters.ts";
import type { GodViewFrame } from "./frameParser.ts";
import { teamColour } from "./shipPresentation.ts";

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Build a minimal GodViewFrame for scoreboard tests. */
function makeFrame(
  scores: Record<string, number>,
  ships: Array<{ id: string; relicsCarried: number }>,
  tick = 0,
  maxTicks = 3600,
): GodViewFrame {
  const makeShip = (id: string, relicsCarried: number) => ({
    id,
    class: "scout",
    alive: true,
    invuln: false,
    pos: { x: 0, y: 0 },
    vel: { x: 0, y: 0 },
    heading: 0,
    angVel: 0,
    hull: { cur: 100, max: 100 },
    shield: { cur: 50, max: 50 },
    aether: { cur: 20, max: 20 },
    sigil: null,
    cannonCooldown: 0,
    relicsCarried,
    afterburnerTicksLeft: 0,
  });

  return {
    type: "godView",
    tick,
    maxTicks,
    seed: 42,
    arena: { width: 1000, height: 1000 },
    ships: ships.map((s) => makeShip(s.id, s.relicsCarried)),
    anchors: [],
    relics: [],
    asteroids: [],
    projectiles: [],
    singularities: [],
    mines: [],
    scores,
  };
}

/** Build a minimal LadderStanding. */
function makeStanding(
  competitor: string,
  conservativeSkill: number,
  matches = 10,
  mu = 25,
  sigma = 8.333,
): LadderStanding {
  return { competitor, mu, sigma, conservativeSkill, matches };
}

// ── Slice 1: timer — tick 0 of 3600 → "2:00" ─────────────────────────────────

describe("formatTimer — slice 1: full match remaining", () => {
  it("tick 0 of 3600 at default 30 ticks/sec → 2:00", () => {
    expect(formatTimer(0, 3600)).toBe("2:00");
  });

  it("tick 0 of 900 at default 30 ticks/sec → 0:30", () => {
    expect(formatTimer(0, 900)).toBe("0:30");
  });

  it("tick 0 of 1800 at default 30 ticks/sec → 1:00", () => {
    expect(formatTimer(0, 1800)).toBe("1:00");
  });
});

// ── Slice 2: timer — mid-match correct remaining ──────────────────────────────

describe("formatTimer — slice 2: mid-match remaining", () => {
  it("tick 900 of 3600 → 1:30 (90 seconds left)", () => {
    // 3600 - 900 = 2700 remaining ticks → 2700/30 = 90 s = 1:30
    expect(formatTimer(900, 3600)).toBe("1:30");
  });

  it("tick 1800 of 3600 → 1:00 (60 seconds left)", () => {
    expect(formatTimer(1800, 3600)).toBe("1:00");
  });

  it("tick 3540 of 3600 → 0:02 (2 seconds left)", () => {
    // (3600 - 3540) / 30 = 2
    expect(formatTimer(3540, 3600)).toBe("0:02");
  });

  it("respects a custom tickRate parameter", () => {
    // 60 tick/sec, tick 0, maxTicks 3600 → 3600/60 = 60 s = 1:00
    expect(formatTimer(0, 3600, 60)).toBe("1:00");
  });
});

// ── Slice 3: timer — at/over maxTicks → "0:00" ───────────────────────────────

describe("formatTimer — slice 3: expired match", () => {
  it("tick exactly at maxTicks → 0:00", () => {
    expect(formatTimer(3600, 3600)).toBe("0:00");
  });

  it("tick beyond maxTicks is clamped to 0:00", () => {
    expect(formatTimer(3700, 3600)).toBe("0:00");
    expect(formatTimer(99999, 3600)).toBe("0:00");
  });
});

// ── Slice 4: timer — seconds leading zero formatting ─────────────────────────

describe("formatTimer — slice 4: leading-zero seconds", () => {
  it("65 seconds remaining → 1:05 (not 1:5)", () => {
    // Need (maxTicks - tick) / 30 = 65  →  tick = maxTicks - 65*30
    const maxTicks = 3600;
    const tick = maxTicks - 65 * 30; // tick = 1650
    expect(formatTimer(tick, maxTicks)).toBe("1:05");
  });

  it("61 seconds remaining → 1:01", () => {
    const maxTicks = 3600;
    const tick = maxTicks - 61 * 30;
    expect(formatTimer(tick, maxTicks)).toBe("1:01");
  });

  it("9 seconds remaining → 0:09", () => {
    const maxTicks = 3600;
    const tick = maxTicks - 9 * 30;
    expect(formatTimer(tick, maxTicks)).toBe("0:09");
  });

  it("single-digit seconds get leading zero across whole range 0–9", () => {
    const maxTicks = 3600;
    for (let s = 0; s <= 9; s++) {
      const tick = maxTicks - s * 30;
      const result = formatTimer(tick, maxTicks);
      if (s === 0) {
        expect(result).toBe("0:00");
      } else {
        expect(result).toBe(`0:0${s}`);
      }
    }
  });
});

// ── Slice 5: scoreboard rows — sorted by score desc with colour + relics ──────

describe("scoreboardRows — slice 5: sorted rows with colour and relics", () => {
  it("returns rows sorted by score descending", () => {
    const frame = makeFrame(
      { alpha: 100, bravo: 200, charlie: 50 },
      [
        { id: "alpha", relicsCarried: 0 },
        { id: "bravo", relicsCarried: 0 },
        { id: "charlie", relicsCarried: 0 },
      ],
    );
    const rows = scoreboardRows(frame);
    expect(rows.map((r) => r.team)).toEqual(["bravo", "alpha", "charlie"]);
    expect(rows.map((r) => r.score)).toEqual([200, 100, 50]);
  });

  it("assigns the team colour from teamColour()", () => {
    const frame = makeFrame({ alpha: 10 }, [{ id: "alpha", relicsCarried: 0 }]);
    const rows = scoreboardRows(frame);
    expect(rows[0]!.colour).toBe(teamColour("alpha"));
  });

  it("sums relicsCarried for the matching ship id", () => {
    const frame = makeFrame(
      { alpha: 50, bravo: 30 },
      [
        { id: "alpha", relicsCarried: 3 },
        { id: "bravo", relicsCarried: 1 },
      ],
    );
    const rows = scoreboardRows(frame);
    const alpha = rows.find((r) => r.team === "alpha")!;
    const bravo = rows.find((r) => r.team === "bravo")!;
    expect(alpha.relicsCarried).toBe(3);
    expect(bravo.relicsCarried).toBe(1);
  });

  it("reports 0 relicsCarried when no matching ship exists in the frame", () => {
    // Ship may be dead/absent but score still tracked
    const frame = makeFrame({ alpha: 50 }, []);
    const rows = scoreboardRows(frame);
    expect(rows[0]!.relicsCarried).toBe(0);
  });
});

// ── Slice 6: scoreboard — tie / empty frame ───────────────────────────────────

describe("scoreboardRows — slice 6: ties and edge cases", () => {
  it("returns an empty array when scores is empty", () => {
    const frame = makeFrame({}, []);
    expect(scoreboardRows(frame)).toEqual([]);
  });

  it("tied scores preserve deterministic order from Object.entries", () => {
    const frame = makeFrame(
      { alpha: 100, bravo: 100 },
      [
        { id: "alpha", relicsCarried: 0 },
        { id: "bravo", relicsCarried: 0 },
      ],
    );
    const rows = scoreboardRows(frame);
    // Both have score 100; sort is stable, order matches Object.entries
    expect(rows.map((r) => r.score)).toEqual([100, 100]);
    expect(rows).toHaveLength(2);
  });

  it("single team returns one row", () => {
    const frame = makeFrame({ solo: 42 }, [{ id: "solo", relicsCarried: 2 }]);
    const rows = scoreboardRows(frame);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ team: "solo", score: 42, relicsCarried: 2 });
  });

  it("does not mutate the original frame", () => {
    const frame = makeFrame(
      { alpha: 100, bravo: 200 },
      [
        { id: "alpha", relicsCarried: 0 },
        { id: "bravo", relicsCarried: 0 },
      ],
    );
    const original = JSON.stringify(frame);
    scoreboardRows(frame);
    expect(JSON.stringify(frame)).toBe(original);
  });
});

// ── Slice 7: ladder rows — formatted and ordered by conservativeSkill ─────────

describe("ladderRows — slice 7: formatted ladder standings", () => {
  it("sorts by conservativeSkill descending", () => {
    const standings = [
      makeStanding("charlie", 15.0),
      makeStanding("alpha", 25.5),
      makeStanding("bravo", 20.3),
    ];
    const rows = ladderRows(standings);
    expect(rows.map((r) => r.competitor)).toEqual(["alpha", "bravo", "charlie"]);
  });

  it("formats conservativeSkill to one decimal place", () => {
    const standings = [
      makeStanding("alpha", 25.666),
      makeStanding("bravo", 20.0),
      makeStanding("charlie", 10.123),
    ];
    const rows = ladderRows(standings);
    expect(rows[0]!.conservativeSkill).toBe("25.7");
    expect(rows[1]!.conservativeSkill).toBe("20.0");
    expect(rows[2]!.conservativeSkill).toBe("10.1");
  });

  it("preserves matches count unchanged", () => {
    const standings = [makeStanding("alpha", 25, 42)];
    const rows = ladderRows(standings);
    expect(rows[0]!.matches).toBe(42);
  });

  it("returns empty array for empty standings", () => {
    expect(ladderRows([])).toEqual([]);
  });

  it("does not mutate the input array", () => {
    const standings = [makeStanding("b", 10), makeStanding("a", 20)];
    const copy = standings.map((s) => ({ ...s }));
    ladderRows(standings);
    expect(standings[0]!.competitor).toBe(copy[0]!.competitor);
    expect(standings[1]!.competitor).toBe(copy[1]!.competitor);
  });

  it("handles a single entry", () => {
    const rows = ladderRows([makeStanding("solo", 18.75, 5)]);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({
      competitor: "solo",
      conservativeSkill: "18.8",
      matches: 5,
    });
  });

  it("already-sorted input produces same order (idempotent)", () => {
    const standings = [
      makeStanding("first", 30),
      makeStanding("second", 20),
      makeStanding("third", 10),
    ];
    const rows = ladderRows(standings);
    expect(rows.map((r) => r.competitor)).toEqual(["first", "second", "third"]);
  });
});
