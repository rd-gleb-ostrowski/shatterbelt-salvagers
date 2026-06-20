/**
 * Unit tests for soundCues — pure event-driven frame → SoundCue[] mapping.
 *
 * All slices test the REWRITTEN `deriveSoundCues(frame)` — single frame,
 * no delta reconstruction. Events come from `frame.events` (authoritative
 * god-view event stream). Thrust comes from authoritative ship state.
 *
 * Slices:
 *   1. `cannonFired` event → cannon cue
 *   2. `died` event → explosion cue at ship pos
 *   3. `mineDetonated` event → explosion cue at event pos
 *   4. `sigilDischarged` event → sigilDischarge cue with `sigil = which`
 *   5. `relicTaken` event → relicPickup cue
 *   6. `relicBanked` event → relicBank cue
 *   7. `lanceTookHull` event → lanceZap cue
 *   8. matchStart — tick === 0
 *   9. matchEnd — tick >= maxTicks
 *  10. quiet frame (no events, mid-match, stationary ships) → only thrust cues
 *  11. thrust from authoritative ship state
 */

import { describe, it, expect } from "vitest";
import { deriveSoundCues } from "./soundCues.ts";
import type { GodViewFrame, GodShipView, GodEvent } from "./frameParser.ts";

// ── Minimal fixtures ──────────────────────────────────────────────────────────

function makeShip(
  id: string,
  overrides: Partial<GodShipView> = {},
): GodShipView {
  return {
    id,
    class: "Scout",
    alive: true,
    invuln: false,
    pos: { x: 100, y: 200 },
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
    ...overrides,
  };
}

function makeFrame(
  tick: number,
  ships: GodShipView[] = [],
  events: GodEvent[] = [],
  overrides: Partial<GodViewFrame> = {},
): GodViewFrame {
  return {
    type: "godView",
    tick,
    maxTicks: 100,
    seed: 42,
    arena: { width: 1000, height: 1000 },
    ships,
    anchors: [],
    relics: [],
    asteroids: [],
    projectiles: [],
    singularities: [],
    mines: [],
    scores: {},
    events,
    ...overrides,
  };
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("deriveSoundCues", () => {

  // ── slice 1: cannonFired → cannon cue ────────────────────────────────────

  describe("slice 1 — cannonFired event → cannon cue", () => {
    it("emits a cannon cue for a cannonFired event", () => {
      const ship = makeShip("alpha", { pos: { x: 300, y: 400 } });
      const frame = makeFrame(5, [ship], [{ ship: "alpha", event: "cannonFired" }]);

      const cues = deriveSoundCues(frame);
      const cannon = cues.filter((c) => c.kind === "cannon");
      expect(cannon).toHaveLength(1);
      expect(cannon[0]!.shipId).toBe("alpha");
      expect(cannon[0]!.pos).toEqual({ x: 300, y: 400 });
    });

    it("emits one cannon cue per cannonFired event", () => {
      const ships = [makeShip("alpha"), makeShip("beta")];
      const events: GodEvent[] = [
        { ship: "alpha", event: "cannonFired" },
        { ship: "beta", event: "cannonFired" },
      ];
      const frame = makeFrame(5, ships, events);

      const cannon = deriveSoundCues(frame).filter((c) => c.kind === "cannon");
      expect(cannon).toHaveLength(2);
      expect(cannon.map((c) => c.shipId).sort()).toEqual(["alpha", "beta"]);
    });

    it("emits no cannon cue when events array is empty", () => {
      const frame = makeFrame(5, [makeShip("alpha")], []);
      expect(deriveSoundCues(frame).filter((c) => c.kind === "cannon")).toHaveLength(0);
    });
  });

  // ── slice 2: died event → explosion cue ──────────────────────────────────

  describe("slice 2 — died event → explosion cue at ship pos", () => {
    it("emits an explosion cue for a died event with ship pos lookup", () => {
      const ship = makeShip("alpha", { pos: { x: 55, y: 62 } });
      const frame = makeFrame(5, [ship], [{ ship: "alpha", event: "died", by: null }]);

      const cues = deriveSoundCues(frame);
      const exp = cues.filter((c) => c.kind === "explosion");
      expect(exp).toHaveLength(1);
      expect(exp[0]!.shipId).toBe("alpha");
      expect(exp[0]!.pos).toEqual({ x: 55, y: 62 });
    });

    it("emits explosion cues for multiple simultaneous died events", () => {
      const ships = [makeShip("alpha"), makeShip("beta")];
      const events: GodEvent[] = [
        { ship: "alpha", event: "died", by: null },
        { ship: "beta", event: "died", by: "alpha" },
      ];
      const frame = makeFrame(5, ships, events);

      const exp = deriveSoundCues(frame).filter((c) => c.kind === "explosion");
      expect(exp).toHaveLength(2);
      expect(exp.map((c) => c.shipId).sort()).toEqual(["alpha", "beta"]);
    });

    it("emits explosion even if ship is absent from ships list (pos undefined)", () => {
      const frame = makeFrame(5, [], [{ ship: "ghost", event: "died", by: null }]);
      const exp = deriveSoundCues(frame).filter((c) => c.kind === "explosion");
      expect(exp).toHaveLength(1);
      expect(exp[0]!.shipId).toBe("ghost");
    });
  });

  // ── slice 3: mineDetonated event → explosion cue ─────────────────────────

  describe("slice 3 — mineDetonated event → explosion cue at event pos", () => {
    it("emits an explosion cue at the mine's pos from the event payload", () => {
      const frame = makeFrame(5, [], [{
        ship: "alpha",
        event: "mineDetonated",
        mineId: "mine-1",
        pos: { x: 700, y: 500 },
      }]);

      const exp = deriveSoundCues(frame).filter((c) => c.kind === "explosion");
      expect(exp).toHaveLength(1);
      expect(exp[0]!.pos).toEqual({ x: 700, y: 500 });
    });

    it("mineDetonated cue has no shipId (ship is the deployer, not positional)", () => {
      const frame = makeFrame(5, [], [{
        ship: "alpha",
        event: "mineDetonated",
        mineId: "mine-1",
        pos: { x: 100, y: 100 },
      }]);

      const exp = deriveSoundCues(frame).filter((c) => c.kind === "explosion");
      expect(exp[0]!.shipId).toBeUndefined();
    });
  });

  // ── slice 4: sigilDischarged event → sigilDischarge cue ──────────────────

  describe("slice 4 — sigilDischarged event → sigilDischarge cue", () => {
    it("emits a sigilDischarge cue with the which field as sigil", () => {
      const ship = makeShip("alpha");
      const frame = makeFrame(5, [ship], [{
        ship: "alpha",
        event: "sigilDischarged",
        which: "Afterburner",
      }]);

      const cues = deriveSoundCues(frame).filter((c) => c.kind === "sigilDischarge");
      expect(cues).toHaveLength(1);
      expect(cues[0]!.sigil).toBe("Afterburner");
      expect(cues[0]!.shipId).toBe("alpha");
    });

    it("carries the sigil type for different Sigil types", () => {
      const ship = makeShip("alpha");
      const frame = makeFrame(5, [ship], [{
        ship: "alpha",
        event: "sigilDischarged",
        which: "Singularity",
      }]);

      expect(deriveSoundCues(frame).find((c) => c.kind === "sigilDischarge")?.sigil)
        .toBe("Singularity");
    });
  });

  // ── slice 5: relicTaken event → relicPickup cue ───────────────────────────

  describe("slice 5 — relicTaken event → relicPickup cue", () => {
    it("emits a relicPickup cue for a relicTaken event", () => {
      const ship = makeShip("alpha", { pos: { x: 200, y: 300 } });
      const frame = makeFrame(5, [ship], [{ ship: "alpha", event: "relicTaken" }]);

      const cues = deriveSoundCues(frame).filter((c) => c.kind === "relicPickup");
      expect(cues).toHaveLength(1);
      expect(cues[0]!.shipId).toBe("alpha");
      expect(cues[0]!.pos).toEqual({ x: 200, y: 300 });
    });
  });

  // ── slice 6: relicBanked event → relicBank cue ────────────────────────────

  describe("slice 6 — relicBanked event → relicBank cue", () => {
    it("emits a relicBank cue for a relicBanked event", () => {
      const ship = makeShip("alpha");
      const frame = makeFrame(5, [ship], [{
        ship: "alpha",
        event: "relicBanked",
        value: 2,
      }]);

      const cues = deriveSoundCues(frame).filter((c) => c.kind === "relicBank");
      expect(cues).toHaveLength(1);
      expect(cues[0]!.shipId).toBe("alpha");
    });
  });

  // ── slice 7: lanceTookHull event → lanceZap cue ───────────────────────────

  describe("slice 7 — lanceTookHull event → lanceZap cue", () => {
    it("emits a lanceZap cue for a lanceTookHull event", () => {
      const target = makeShip("alpha", { pos: { x: 400, y: 500 } });
      const attacker = makeShip("beta");
      const frame = makeFrame(5, [target, attacker], [{
        ship: "alpha",
        event: "lanceTookHull",
        amount: 30,
        by: "beta",
      }]);

      const cues = deriveSoundCues(frame).filter((c) => c.kind === "lanceZap");
      expect(cues).toHaveLength(1);
      expect(cues[0]!.shipId).toBe("alpha");
      expect(cues[0]!.pos).toEqual({ x: 400, y: 500 });
    });
  });

  // ── slice 8: matchStart cue ───────────────────────────────────────────────

  describe("slice 8 — tick === 0 → matchStart cue", () => {
    it("emits a matchStart cue when tick === 0", () => {
      const frame = makeFrame(0);
      expect(deriveSoundCues(frame).filter((c) => c.kind === "matchStart")).toHaveLength(1);
    });

    it("does NOT emit a matchStart cue mid-match", () => {
      const frame = makeFrame(5);
      expect(deriveSoundCues(frame).filter((c) => c.kind === "matchStart")).toHaveLength(0);
    });
  });

  // ── slice 9: matchEnd cue ─────────────────────────────────────────────────

  describe("slice 9 — tick >= maxTicks → matchEnd cue", () => {
    it("emits a matchEnd cue when tick === maxTicks", () => {
      const frame = makeFrame(100, [], [], { maxTicks: 100 });
      expect(deriveSoundCues(frame).filter((c) => c.kind === "matchEnd")).toHaveLength(1);
    });

    it("emits a matchEnd cue when tick > maxTicks", () => {
      const frame = makeFrame(101, [], [], { maxTicks: 100 });
      expect(deriveSoundCues(frame).filter((c) => c.kind === "matchEnd")).toHaveLength(1);
    });

    it("does NOT emit a matchEnd cue mid-match", () => {
      const frame = makeFrame(50, [], [], { maxTicks: 100 });
      expect(deriveSoundCues(frame).filter((c) => c.kind === "matchEnd")).toHaveLength(0);
    });
  });

  // ── slice 10: quiet frame → no cues (except thrust if thrusting) ─────────

  describe("slice 10 — quiet frame → only thrust cues from state", () => {
    it("returns empty cue list for mid-match frame with no events and stationary ships", () => {
      const ship = makeShip("alpha", {
        vel: { x: 0, y: 0 },
        afterburnerTicksLeft: 0,
      });
      const frame = makeFrame(5, [ship], []);
      const nonThrust = deriveSoundCues(frame).filter((c) => c.kind !== "thrust");
      expect(nonThrust).toHaveLength(0);
    });

    it("returns completely empty array for mid-match frame with no ships and no events", () => {
      const frame = makeFrame(5, [], []);
      expect(deriveSoundCues(frame)).toHaveLength(0);
    });
  });

  // ── slice 11: thrust cues from authoritative ship state ───────────────────

  describe("slice 11 — thrust cues from authoritative ship state", () => {
    it("emits a thrust cue for a ship with afterburnerTicksLeft > 0", () => {
      const frame = makeFrame(5, [makeShip("alpha", { afterburnerTicksLeft: 3 })]);
      const thrust = deriveSoundCues(frame).filter((c) => c.kind === "thrust");
      expect(thrust).toHaveLength(1);
      expect(thrust[0]!.shipId).toBe("alpha");
    });

    it("does not emit a thrust cue for a dead ship", () => {
      const frame = makeFrame(5, [
        makeShip("alpha", { alive: false, afterburnerTicksLeft: 3 }),
      ]);
      expect(deriveSoundCues(frame).filter((c) => c.kind === "thrust")).toHaveLength(0);
    });

    it("does not emit a thrust cue for a stationary ship", () => {
      const frame = makeFrame(5, [
        makeShip("alpha", { vel: { x: 0, y: 0 }, afterburnerTicksLeft: 0 }),
      ]);
      expect(deriveSoundCues(frame).filter((c) => c.kind === "thrust")).toHaveLength(0);
    });
  });
});
