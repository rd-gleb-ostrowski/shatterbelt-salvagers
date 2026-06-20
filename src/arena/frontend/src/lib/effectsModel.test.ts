/**
 * Unit tests for effectsModel — pure transient-effect aging model.
 *
 * Slices:
 *   6a — addExplosions spawns explosion TransientEffects
 *   6b — advanceEffects ages and expires effects
 *   6c — spawnEffectsFromEvents: died → explosion, mineDetonated → explosion,
 *         lanceTookHull → arcLance beam, other events → no effect
 */

import { describe, it, expect } from "vitest";
import {
  EMPTY_EFFECTS,
  EXPLOSION_LIFETIME_TICKS,
  ARC_LANCE_LIFETIME_TICKS,
  addExplosions,
  advanceEffects,
  spawnEffectsFromEvents,
} from "./effectsModel.ts";
import type { GodEvent, GodShipView } from "./frameParser.ts";

// ── Helpers ───────────────────────────────────────────────────────────────────

function makeShip(id: string, x = 100, y = 200): GodShipView {
  return {
    id,
    class: "Scout",
    alive: true,
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

// ── slice 6a: addExplosions ───────────────────────────────────────────────────

describe("effectsModel", () => {
  describe("slice 6a — addExplosions spawns transient effects", () => {
    it("adds one explosion effect per input", () => {
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

  // ── slice 6b: advanceEffects ──────────────────────────────────────────────

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

  // ── slice 6c: spawnEffectsFromEvents ─────────────────────────────────────

  describe("slice 6c — spawnEffectsFromEvents maps events to TransientEffects", () => {
    it("died event → explosion at ship pos", () => {
      const ship = makeShip("alpha", 300, 400);
      const events: GodEvent[] = [{ ship: "alpha", event: "died", by: null }];
      const state = spawnEffectsFromEvents(EMPTY_EFFECTS, events, [ship]);

      expect(state.effects).toHaveLength(1);
      expect(state.effects[0]!.kind).toBe("explosion");
      expect(state.effects[0]!.pos).toEqual({ x: 300, y: 400 });
      expect(state.effects[0]!.ticksLeft).toBe(EXPLOSION_LIFETIME_TICKS);
    });

    it("mineDetonated event → explosion at event pos, not ship pos", () => {
      const ship = makeShip("alpha", 100, 100);
      const events: GodEvent[] = [{
        ship: "alpha",
        event: "mineDetonated",
        mineId: "mine-1",
        pos: { x: 700, y: 500 },
      }];
      const state = spawnEffectsFromEvents(EMPTY_EFFECTS, events, [ship]);

      expect(state.effects).toHaveLength(1);
      expect(state.effects[0]!.kind).toBe("explosion");
      expect(state.effects[0]!.pos).toEqual({ x: 700, y: 500 });
    });

    it("lanceTookHull event → arcLance beam from attacker to target", () => {
      const target = makeShip("alpha", 400, 500);
      const attacker = makeShip("beta", 200, 300);
      const events: GodEvent[] = [{
        ship: "alpha",
        event: "lanceTookHull",
        amount: 30,
        by: "beta",
      }];
      const state = spawnEffectsFromEvents(EMPTY_EFFECTS, events, [target, attacker]);

      expect(state.effects).toHaveLength(1);
      const fx = state.effects[0]!;
      expect(fx.kind).toBe("arcLance");
      if (fx.kind === "arcLance") {
        expect(fx.from).toEqual({ x: 200, y: 300 }); // attacker
        expect(fx.to).toEqual({ x: 400, y: 500 });   // target
        expect(fx.pos).toEqual({ x: 300, y: 400 });   // midpoint
        expect(fx.ticksLeft).toBe(ARC_LANCE_LIFETIME_TICKS);
      }
    });

    it("lanceTookHull with missing attacker ship → no effect spawned", () => {
      const target = makeShip("alpha", 400, 500);
      const events: GodEvent[] = [{
        ship: "alpha",
        event: "lanceTookHull",
        amount: 30,
        by: "missing-ship",
      }];
      const state = spawnEffectsFromEvents(EMPTY_EFFECTS, events, [target]);

      expect(state.effects).toHaveLength(0);
    });

    it("non-visual events (tookHull, cannonFired, etc.) produce no effects", () => {
      const ship = makeShip("alpha");
      const events: GodEvent[] = [
        { ship: "alpha", event: "tookHull", amount: 10, by: "beta" },
        { ship: "alpha", event: "cannonFired" },
        { ship: "alpha", event: "relicTaken" },
        { ship: "alpha", event: "shieldDown" },
      ];
      const state = spawnEffectsFromEvents(EMPTY_EFFECTS, events, [ship]);

      expect(state.effects).toHaveLength(0);
    });

    it("returns the same state when events array is empty", () => {
      const state = spawnEffectsFromEvents(EMPTY_EFFECTS, [], []);
      expect(state).toBe(EMPTY_EFFECTS);
    });

    it("multiple events in one frame spawn multiple effects", () => {
      const alpha = makeShip("alpha", 100, 100);
      const beta = makeShip("beta", 200, 200);
      const events: GodEvent[] = [
        { ship: "alpha", event: "died", by: "beta" },
        { ship: "beta", event: "mineDetonated", mineId: "m1", pos: { x: 500, y: 500 } },
      ];
      const state = spawnEffectsFromEvents(EMPTY_EFFECTS, events, [alpha, beta]);

      expect(state.effects).toHaveLength(2);
      expect(state.effects.every((e) => e.kind === "explosion")).toBe(true);
    });

    it("arcLance effects age and expire via advanceEffects", () => {
      const target = makeShip("alpha", 400, 500);
      const attacker = makeShip("beta", 200, 300);
      const events: GodEvent[] = [{
        ship: "alpha",
        event: "lanceTookHull",
        amount: 30,
        by: "beta",
      }];
      let state = spawnEffectsFromEvents(EMPTY_EFFECTS, events, [target, attacker]);
      expect(state.effects).toHaveLength(1);

      state = advanceEffects(state, ARC_LANCE_LIFETIME_TICKS);
      expect(state.effects).toHaveLength(0);
    });
  });
});
