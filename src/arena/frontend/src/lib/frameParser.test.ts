import { describe, it, expect } from "vitest";
import { parseGodViewFrame } from "./frameParser.ts";

// ── Fixtures ──────────────────────────────────────────────────────────────────

/** Minimal valid god-view frame matching the server's GodViewFrameJson shape. */
const MINIMAL_FRAME = {
  type: "godView",
  tick: 42,
  maxTicks: 3600,
  seed: 99887766,
  arena: { width: 2000, height: 1500 },
  ships: [],
  anchors: [],
  relics: [],
  asteroids: [],
  projectiles: [],
  singularities: [],
  mines: [],
  scores: {},
  events: [],
};

/** A full frame with one of every entity type. */
const FULL_FRAME = {
  type: "godView",
  tick: 100,
  maxTicks: 7200,
  seed: 12345678,
  arena: { width: 1600, height: 900 },
  ships: [
    {
      id: "ship-alpha",
      class: "scout",
      alive: true,
      invuln: false,
      pos: { x: 400, y: 300 },
      vel: { x: 1.5, y: -0.5 },
      heading: 1.57,
      angVel: 0.01,
      hull: { cur: 80, max: 100 },
      shield: { cur: 50, max: 50 },
      aether: { cur: 30, max: 60 },
      sigil: "singularity",
      cannonCooldown: 0,
      relicsCarried: 2,
      afterburnerTicksLeft: 0,
    },
    {
      id: "ship-beta",
      class: "brawler",
      alive: false,
      invuln: true,
      pos: { x: 800, y: 600 },
      vel: { x: 0, y: 0 },
      heading: 3.14,
      angVel: 0,
      hull: { cur: 0, max: 120 },
      shield: { cur: 0, max: 80 },
      aether: { cur: 10, max: 60 },
      sigil: null,
      cannonCooldown: 5,
      relicsCarried: 0,
      afterburnerTicksLeft: 12,
    },
  ],
  anchors: [
    { shipId: "ship-alpha", pos: { x: 100, y: 100 } },
    { shipId: "ship-beta", pos: { x: 1500, y: 800 } },
  ],
  relics: [
    { id: "relic-1", pos: { x: 700, y: 450 }, vel: { x: 0.1, y: 0 }, value: 1 },
  ],
  asteroids: [
    { id: "ast-0", pos: { x: 300, y: 200 }, vel: { x: -0.2, y: 0.1 }, radius: 40 },
  ],
  projectiles: [
    { id: "proj-0", pos: { x: 410, y: 310 }, vel: { x: 5, y: 0 }, owner: "ship-alpha" },
  ],
  singularities: [
    { id: "sing-0", pos: { x: 600, y: 400 }, radius: 80, ticksLeft: 30 },
  ],
  mines: [
    { id: "mine-0", pos: { x: 200, y: 700 }, own: false },
  ],
  scores: { "ship-alpha": 3, "ship-beta": 1 },
  events: [
    { ship: "ship-alpha", event: "cannonFired" },
    { ship: "ship-beta", event: "died", by: "ship-alpha" },
  ],
};

// ── parseGodViewFrame: rejection cases ───────────────────────────────────────

describe("parseGodViewFrame — rejects invalid input", () => {
  it("returns null for null", () => {
    expect(parseGodViewFrame(null)).toBeNull();
  });

  it("returns null for a string", () => {
    expect(parseGodViewFrame("godView")).toBeNull();
  });

  it("returns null when type is missing", () => {
    const { type: _omit, ...rest } = MINIMAL_FRAME;
    expect(parseGodViewFrame(rest)).toBeNull();
  });

  it("returns null when type is wrong", () => {
    expect(parseGodViewFrame({ ...MINIMAL_FRAME, type: "tick" })).toBeNull();
  });

  it("returns null when tick is not a number", () => {
    expect(parseGodViewFrame({ ...MINIMAL_FRAME, tick: "42" })).toBeNull();
  });

  it("returns null when arena is malformed", () => {
    expect(parseGodViewFrame({ ...MINIMAL_FRAME, arena: { width: "bad" } })).toBeNull();
  });

  it("returns null when a ship has a missing pos", () => {
    const bad = {
      ...MINIMAL_FRAME,
      ships: [{ ...FULL_FRAME.ships[0], pos: undefined }],
    };
    expect(parseGodViewFrame(bad)).toBeNull();
  });

  it("returns null when scores has a non-number value", () => {
    expect(
      parseGodViewFrame({ ...MINIMAL_FRAME, scores: { shipA: "ten" } })
    ).toBeNull();
  });
});

// ── parseGodViewFrame: acceptance cases ──────────────────────────────────────

describe("parseGodViewFrame — accepts valid frames", () => {
  it("parses a minimal frame and returns typed GodViewFrame", () => {
    const frame = parseGodViewFrame(MINIMAL_FRAME);
    expect(frame).not.toBeNull();
    expect(frame!.type).toBe("godView");
    expect(frame!.tick).toBe(42);
    expect(frame!.maxTicks).toBe(3600);
    expect(frame!.seed).toBe(99887766);
    expect(frame!.arena).toEqual({ width: 2000, height: 1500 });
    expect(frame!.ships).toHaveLength(0);
    expect(frame!.scores).toEqual({});
    expect(frame!.events).toEqual([]);
  });

  it("parses a full frame with all entity types", () => {
    const frame = parseGodViewFrame(FULL_FRAME);
    expect(frame).not.toBeNull();
    expect(frame!.tick).toBe(100);
    expect(frame!.ships).toHaveLength(2);
    expect(frame!.anchors).toHaveLength(2);
    expect(frame!.relics).toHaveLength(1);
    expect(frame!.asteroids).toHaveLength(1);
    expect(frame!.projectiles).toHaveLength(1);
    expect(frame!.singularities).toHaveLength(1);
    expect(frame!.mines).toHaveLength(1);
  });

  it("maps ship fields to the exact camelCase wire names", () => {
    const frame = parseGodViewFrame(FULL_FRAME)!;
    const ship = frame.ships[0];
    expect(ship.id).toBe("ship-alpha");
    expect(ship.angVel).toBeCloseTo(0.01);
    expect(ship.cannonCooldown).toBe(0);
    expect(ship.relicsCarried).toBe(2);
    expect(ship.afterburnerTicksLeft).toBe(0);
    expect(ship.sigil).toBe("singularity");
    expect(ship.invuln).toBe(false);
  });

  it("preserves sigil: null correctly", () => {
    const frame = parseGodViewFrame(FULL_FRAME)!;
    expect(frame.ships[1].sigil).toBeNull();
  });

  it("maps anchor shipId (camelCase) from wire", () => {
    const frame = parseGodViewFrame(FULL_FRAME)!;
    expect(frame.anchors[0].shipId).toBe("ship-alpha");
  });

  it("maps singularity ticksLeft (camelCase) from wire", () => {
    const frame = parseGodViewFrame(FULL_FRAME)!;
    expect(frame.singularities[0].ticksLeft).toBe(30);
  });

  it("parses scores as Record<string, number>", () => {
    const frame = parseGodViewFrame(FULL_FRAME)!;
    expect(frame.scores["ship-alpha"]).toBe(3);
    expect(frame.scores["ship-beta"]).toBe(1);
  });

  it("round-trips through JSON.parse(JSON.stringify(frame))", () => {
    const frame = parseGodViewFrame(FULL_FRAME)!;
    const reparse = parseGodViewFrame(JSON.parse(JSON.stringify(frame)));
    expect(reparse).not.toBeNull();
    expect(reparse!.tick).toBe(frame.tick);
    expect(reparse!.ships).toHaveLength(2);
    expect(reparse!.events).toHaveLength(2);
  });
});

// ── parseGodViewFrame: events array ─────────────────────────────────────────

describe("parseGodViewFrame — events array", () => {
  it("parses a frame with no events field as events: []", () => {
    const { events: _omit, ...rest } = MINIMAL_FRAME;
    const frame = parseGodViewFrame(rest);
    expect(frame).not.toBeNull();
    expect(frame!.events).toEqual([]);
  });

  it("parses a frame with an empty events array as events: []", () => {
    const frame = parseGodViewFrame({ ...MINIMAL_FRAME, events: [] });
    expect(frame).not.toBeNull();
    expect(frame!.events).toHaveLength(0);
  });

  it("parses a cannonFired event (no payload beyond ship + event)", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [{ ship: "alpha", event: "cannonFired" }],
    });
    expect(frame!.events).toHaveLength(1);
    expect(frame!.events[0]).toEqual({ ship: "alpha", event: "cannonFired" });
  });

  it("parses a died event with by: null", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [{ ship: "alpha", event: "died", by: null }],
    });
    expect(frame!.events[0]).toEqual({ ship: "alpha", event: "died", by: null });
  });

  it("parses a died event with by: string (attacker id)", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [{ ship: "alpha", event: "died", by: "beta" }],
    });
    expect(frame!.events[0]).toEqual({ ship: "alpha", event: "died", by: "beta" });
  });

  it("parses lanceTookHull with amount and by", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [{ ship: "alpha", event: "lanceTookHull", amount: 40, by: "beta" }],
    });
    expect(frame!.events[0]).toEqual({
      ship: "alpha",
      event: "lanceTookHull",
      amount: 40,
      by: "beta",
    });
  });

  it("parses mineDetonated with mineId and pos", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [{ ship: "alpha", event: "mineDetonated", mineId: "m-1", pos: { x: 300, y: 400 } }],
    });
    expect(frame!.events[0]).toEqual({
      ship: "alpha",
      event: "mineDetonated",
      mineId: "m-1",
      pos: { x: 300, y: 400 },
    });
  });

  it("parses sigilDischarged with which field", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [{ ship: "alpha", event: "sigilDischarged", which: "ArcLance" }],
    });
    expect(frame!.events[0]).toEqual({
      ship: "alpha",
      event: "sigilDischarged",
      which: "ArcLance",
    });
  });

  it("parses relicBanked with value", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [{ ship: "alpha", event: "relicBanked", value: 3 }],
    });
    expect(frame!.events[0]).toEqual({ ship: "alpha", event: "relicBanked", value: 3 });
  });

  it("skips malformed event entries but still parses rest of frame", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [
        { ship: "alpha", event: "cannonFired" },         // valid
        { ship: 42, event: "cannonFired" },              // malformed: ship not string
        { ship: "beta", event: "unknown-tag" },          // unknown tag — skipped
        { ship: "beta", event: "died", by: null },       // valid
      ],
    });
    expect(frame).not.toBeNull();
    expect(frame!.events).toHaveLength(2);
    expect(frame!.events[0]!.event).toBe("cannonFired");
    expect(frame!.events[1]!.event).toBe("died");
  });

  it("parses relicDropped with relicId and pos", () => {
    const frame = parseGodViewFrame({
      ...MINIMAL_FRAME,
      events: [{
        ship: "alpha",
        event: "relicDropped",
        relicId: "r-1",
        pos: { x: 100, y: 200 },
      }],
    });
    expect(frame!.events[0]).toEqual({
      ship: "alpha",
      event: "relicDropped",
      relicId: "r-1",
      pos: { x: 100, y: 200 },
    });
  });

  it("parses a full frame with two events from FULL_FRAME fixture", () => {
    const frame = parseGodViewFrame(FULL_FRAME);
    expect(frame).not.toBeNull();
    expect(frame!.events).toHaveLength(2);
    expect(frame!.events[0]).toEqual({ ship: "ship-alpha", event: "cannonFired" });
    expect(frame!.events[1]).toEqual({ ship: "ship-beta", event: "died", by: "ship-alpha" });
  });
});
