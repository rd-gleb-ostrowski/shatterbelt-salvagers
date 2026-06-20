/**
 * Unit tests for explosionDetector — pure frame-delta logic.
 *
 * All six tracer slices from the PRD:
 *   1. alive → not-alive   → explosion detected
 *   2. alive → absent       → explosion detected
 *   3. stays alive          → no explosion
 *   4. stays dead           → no new explosion
 *   5. dead → alive (respawn) → no explosion
 *   6. (effectsModel) transient-effect aging/expiry
 */

import { describe, it, expect } from "vitest";
import { detectExplosions } from "./explosionDetector.ts";
import type { GodShipView } from "./frameParser.ts";
import {
  EMPTY_EFFECTS,
  EXPLOSION_LIFETIME_TICKS,
  addExplosions,
  advanceEffects,
} from "./effectsModel.ts";

// ── Minimal ship fixture ──────────────────────────────────────────────────────

function makeShip(
  id: string,
  alive: boolean,
  x = 100,
  y = 200,
): GodShipView {
  return {
    id,
    class: "Scout",
    alive,
    invuln: false,
    pos: { x, y },
    vel: { x: 0, y: 0 },
    heading: 0,
    angVel: 0,
    hull: { cur: 100, max: 100 },
    shield: { cur: 50, max: 50 },
    aether: { cur: 20, max: 20 },
    sigil: null,
    cannonCooldown: 0,
    relicsCarried: 0,
    afterburnerTicksLeft: 0,
  };
}

// ── detectExplosions ──────────────────────────────────────────────────────────

describe("detectExplosions", () => {
  describe("slice 1 — alive last frame, not-alive this frame → explosion", () => {
    it("detects a single ship that transitioned alive → dead", () => {
      const prev = [makeShip("alpha", true, 300, 400)];
      const curr = [makeShip("alpha", false, 305, 402)];

      const events = detectExplosions(prev, curr);

      expect(events).toHaveLength(1);
      expect(events[0]!.shipId).toBe("alpha");
      // Position should be the CURRENT (death) position
      expect(events[0]!.pos).toEqual({ x: 305, y: 402 });
    });

    it("detects multiple simultaneous deaths", () => {
      const prev = [makeShip("alpha", true), makeShip("beta", true)];
      const curr = [makeShip("alpha", false), makeShip("beta", false)];

      const events = detectExplosions(prev, curr);

      expect(events).toHaveLength(2);
      const ids = events.map((e) => e.shipId).sort();
      expect(ids).toEqual(["alpha", "beta"]);
    });
  });

  describe("slice 2 — alive last frame, absent this frame → explosion", () => {
    it("detects a ship that was alive but is now absent (removed from frame)", () => {
      const prev = [makeShip("alpha", true, 50, 60)];
      const curr: GodShipView[] = []; // alpha is gone

      const events = detectExplosions(prev, curr);

      expect(events).toHaveLength(1);
      expect(events[0]!.shipId).toBe("alpha");
      // Position must be the LAST KNOWN (prev) position since it's absent
      expect(events[0]!.pos).toEqual({ x: 50, y: 60 });
    });

    it("handles a mix: one absent + one still alive", () => {
      const prev = [makeShip("alpha", true), makeShip("beta", true)];
      const curr = [makeShip("beta", true)]; // alpha absent, beta alive

      const events = detectExplosions(prev, curr);

      expect(events).toHaveLength(1);
      expect(events[0]!.shipId).toBe("alpha");
    });
  });

  describe("slice 3 — ship stays alive → no explosion", () => {
    it("returns empty when a ship is alive in both frames", () => {
      const prev = [makeShip("alpha", true)];
      const curr = [makeShip("alpha", true)];

      expect(detectExplosions(prev, curr)).toHaveLength(0);
    });

    it("returns empty when all ships stay alive", () => {
      const prev = [makeShip("alpha", true), makeShip("beta", true)];
      const curr = [makeShip("alpha", true), makeShip("beta", true)];

      expect(detectExplosions(prev, curr)).toHaveLength(0);
    });

    it("returns empty when both frames are empty", () => {
      expect(detectExplosions([], [])).toHaveLength(0);
    });
  });

  describe("slice 4 — ship stays dead across two frames → no new explosion", () => {
    it("does not re-fire an explosion for a ship that was already dead last frame", () => {
      const prev = [makeShip("alpha", false)]; // already dead
      const curr = [makeShip("alpha", false)]; // still dead

      expect(detectExplosions(prev, curr)).toHaveLength(0);
    });

    it("dead ship that disappears between frames also produces no explosion", () => {
      const prev = [makeShip("alpha", false)]; // already dead
      const curr: GodShipView[] = []; // now absent — but was already dead

      expect(detectExplosions(prev, curr)).toHaveLength(0);
    });
  });

  describe("slice 5 — dead last frame, alive this frame (respawn) → no explosion", () => {
    it("does not produce an explosion when a dead ship respawns", () => {
      const prev = [makeShip("alpha", false)];
      const curr = [makeShip("alpha", true)];

      expect(detectExplosions(prev, curr)).toHaveLength(0);
    });

    it("handles concurrent respawn and new death correctly", () => {
      const prev = [makeShip("alpha", false), makeShip("beta", true)];
      const curr = [makeShip("alpha", true), makeShip("beta", false)];

      const events = detectExplosions(prev, curr);

      expect(events).toHaveLength(1);
      expect(events[0]!.shipId).toBe("beta");
    });
  });
});

// ── effectsModel ─────────────────────────────────────────────────────────────

describe("effectsModel", () => {
  describe("slice 6a — addExplosions spawns transient effects", () => {
    it("adds one explosion effect per ExplosionEvent", () => {
      const events = [{ shipId: "alpha", pos: { x: 10, y: 20 } }];
      const state = addExplosions(EMPTY_EFFECTS, events);

      expect(state.effects).toHaveLength(1);
      expect(state.effects[0]!.kind).toBe("explosion");
      expect(state.effects[0]!.pos).toEqual({ x: 10, y: 20 });
      expect(state.effects[0]!.ticksLeft).toBe(EXPLOSION_LIFETIME_TICKS);
    });

    it("assigns unique ids to concurrent explosions", () => {
      const events = [
        { shipId: "alpha", pos: { x: 0, y: 0 } },
        { shipId: "beta", pos: { x: 1, y: 1 } },
      ];
      const state = addExplosions(EMPTY_EFFECTS, events);

      const ids = state.effects.map((e) => e.id);
      expect(new Set(ids).size).toBe(2);
    });

    it("returns the same state when given an empty events list", () => {
      const state = addExplosions(EMPTY_EFFECTS, []);
      expect(state).toBe(EMPTY_EFFECTS);
    });

    it("respects a custom lifetime parameter", () => {
      const events = [{ shipId: "gamma", pos: { x: 0, y: 0 } }];
      const state = addExplosions(EMPTY_EFFECTS, events, 5);

      expect(state.effects[0]!.ticksLeft).toBe(5);
    });
  });

  describe("slice 6b — advanceEffects ages and expires effects", () => {
    it("decrements ticksLeft by 1 per advance call", () => {
      const s0 = addExplosions(EMPTY_EFFECTS, [{ shipId: "a", pos: { x: 0, y: 0 } }]);
      const s1 = advanceEffects(s0);

      expect(s1.effects[0]!.ticksLeft).toBe(EXPLOSION_LIFETIME_TICKS - 1);
    });

    it("removes effects whose ticksLeft reaches 0", () => {
      const s0 = addExplosions(EMPTY_EFFECTS, [{ shipId: "a", pos: { x: 0, y: 0 } }], 1);
      const s1 = advanceEffects(s0); // ticksLeft goes 1 → 0 → removed

      expect(s1.effects).toHaveLength(0);
    });

    it("only removes expired effects, keeping active ones", () => {
      const s0 = addExplosions(EMPTY_EFFECTS, [
        { shipId: "short", pos: { x: 0, y: 0 } },
      ], 1);
      const s1 = addExplosions(s0, [
        { shipId: "long", pos: { x: 1, y: 1 } },
      ], 10);
      const s2 = advanceEffects(s1, 1); // "short" expires, "long" stays

      expect(s2.effects).toHaveLength(1);
      expect(s2.effects[0]!.id).toContain("long");
    });

    it("advances by multiple ticks at once", () => {
      const s0 = addExplosions(EMPTY_EFFECTS, [{ shipId: "a", pos: { x: 0, y: 0 } }], 10);
      const s1 = advanceEffects(s0, 9);

      expect(s1.effects[0]!.ticksLeft).toBe(1);
    });

    it("does not mutate the input state", () => {
      const s0 = addExplosions(EMPTY_EFFECTS, [{ shipId: "a", pos: { x: 0, y: 0 } }]);
      const original = s0.effects[0]!.ticksLeft;
      advanceEffects(s0); // should not affect s0

      expect(s0.effects[0]!.ticksLeft).toBe(original);
    });
  });
});
