/**
 * Unit tests for soundCues — pure frame-delta → SoundCue[] mapping.
 *
 * Eight tracer slices (TDD, red-green order):
 *   1. newly-destroyed ship → explosion cue
 *   2. cannonCooldown 0→>0  → cannon cue; unchanged cooldown → no cue
 *   3. sigil Some→None on living ship → sigil-discharge cue; death case excluded
 *   4. relicsCarried increase → relicPickup cue
 *   5. relicsCarried drop + score increase → relicBank cue
 *   6. tick === 0 (first/new match) → matchStart cue
 *   7. tick crossing maxTicks → matchEnd cue (only once)
 *   8. quiet frame (no deltas) → empty cue list
 */

import { describe, it, expect } from "vitest";
import { deriveSoundCues } from "./soundCues.ts";
import type { GodViewFrame, GodShipView } from "./frameParser.ts";

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
    ...overrides,
  };
}

// ── slice 1: explosion cues ───────────────────────────────────────────────────

describe("deriveSoundCues", () => {
  describe("slice 1 — newly-destroyed ship → explosion cue", () => {
    it("emits one explosion cue when a ship transitions alive → dead", () => {
      const prev = makeFrame(1, [makeShip("alpha", { alive: true })]);
      const curr = makeFrame(2, [makeShip("alpha", { alive: false })]);

      const cues = deriveSoundCues(prev, curr);

      const explosions = cues.filter((c) => c.kind === "explosion");
      expect(explosions).toHaveLength(1);
      expect(explosions[0]!.shipId).toBe("alpha");
    });

    it("emits explosion cues for multiple simultaneous deaths", () => {
      const prev = makeFrame(1, [
        makeShip("alpha", { alive: true }),
        makeShip("beta", { alive: true }),
      ]);
      const curr = makeFrame(2, [
        makeShip("alpha", { alive: false }),
        makeShip("beta", { alive: false }),
      ]);

      const cues = deriveSoundCues(prev, curr);
      const explosionIds = cues
        .filter((c) => c.kind === "explosion")
        .map((c) => c.shipId)
        .sort();

      expect(explosionIds).toEqual(["alpha", "beta"]);
    });

    it("does not emit an explosion for a ship that stays alive", () => {
      const prev = makeFrame(1, [makeShip("alpha", { alive: true })]);
      const curr = makeFrame(2, [makeShip("alpha", { alive: true })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "explosion")).toHaveLength(0);
    });

    it("emits explosion cue with position from the death frame", () => {
      const prev = makeFrame(1, [makeShip("alpha", { alive: true, pos: { x: 50, y: 60 } })]);
      const curr = makeFrame(2, [makeShip("alpha", { alive: false, pos: { x: 55, y: 62 } })]);

      const cues = deriveSoundCues(prev, curr);
      const exp = cues.find((c) => c.kind === "explosion");
      expect(exp?.pos).toEqual({ x: 55, y: 62 });
    });
  });

  // ── slice 2: cannon-fire cues ───────────────────────────────────────────────

  describe("slice 2 — cannonCooldown 0→>0 → cannon cue", () => {
    it("emits a cannon cue when a ship's cannonCooldown goes from 0 to positive", () => {
      const prev = makeFrame(1, [makeShip("alpha", { cannonCooldown: 0 })]);
      const curr = makeFrame(2, [makeShip("alpha", { cannonCooldown: 10 })]);

      const cues = deriveSoundCues(prev, curr);
      const cannons = cues.filter((c) => c.kind === "cannon");
      expect(cannons).toHaveLength(1);
      expect(cannons[0]!.shipId).toBe("alpha");
    });

    it("does not emit a cannon cue when cannonCooldown is already counting down", () => {
      // mid-cooldown: 8 → 7 (no new shot)
      const prev = makeFrame(1, [makeShip("alpha", { cannonCooldown: 8 })]);
      const curr = makeFrame(2, [makeShip("alpha", { cannonCooldown: 7 })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "cannon")).toHaveLength(0);
    });

    it("does not emit a cannon cue when cannonCooldown stays at 0", () => {
      const prev = makeFrame(1, [makeShip("alpha", { cannonCooldown: 0 })]);
      const curr = makeFrame(2, [makeShip("alpha", { cannonCooldown: 0 })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "cannon")).toHaveLength(0);
    });

    it("does not emit a cannon cue for a ship that just died (dead in curr)", () => {
      // Ship fired and died in the same tick — no cannon sound
      const prev = makeFrame(1, [makeShip("alpha", { alive: true, cannonCooldown: 0 })]);
      const curr = makeFrame(2, [makeShip("alpha", { alive: false, cannonCooldown: 10 })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "cannon")).toHaveLength(0);
    });
  });

  // ── slice 3: sigil-discharge cues ──────────────────────────────────────────

  describe("slice 3 — sigil Some→None on living ship → sigil-discharge cue", () => {
    it("emits a sigilDischarge cue when a living ship's sigil disappears", () => {
      const prev = makeFrame(1, [makeShip("alpha", { sigil: "Afterburner" })]);
      const curr = makeFrame(2, [makeShip("alpha", { sigil: null })]);

      const cues = deriveSoundCues(prev, curr);
      const discharges = cues.filter((c) => c.kind === "sigilDischarge");
      expect(discharges).toHaveLength(1);
      expect(discharges[0]!.shipId).toBe("alpha");
      expect(discharges[0]!.sigil).toBe("Afterburner");
    });

    it("carries the sigil type on the cue", () => {
      const prev = makeFrame(1, [makeShip("alpha", { sigil: "Singularity" })]);
      const curr = makeFrame(2, [makeShip("alpha", { sigil: null })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.find((c) => c.kind === "sigilDischarge")?.sigil).toBe("Singularity");
    });

    it("does NOT emit a sigilDischarge cue when the ship died holding a sigil", () => {
      const prev = makeFrame(1, [makeShip("alpha", { alive: true, sigil: "Bulwark" })]);
      const curr = makeFrame(2, [makeShip("alpha", { alive: false, sigil: null })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "sigilDischarge")).toHaveLength(0);
    });

    it("does not emit a cue when the sigil stays the same", () => {
      const prev = makeFrame(1, [makeShip("alpha", { sigil: "ArcLance" })]);
      const curr = makeFrame(2, [makeShip("alpha", { sigil: "ArcLance" })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "sigilDischarge")).toHaveLength(0);
    });

    it("does not emit a cue when sigil stays null", () => {
      const prev = makeFrame(1, [makeShip("alpha", { sigil: null })]);
      const curr = makeFrame(2, [makeShip("alpha", { sigil: null })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "sigilDischarge")).toHaveLength(0);
    });
  });

  // ── slice 4: relicPickup cues ───────────────────────────────────────────────

  describe("slice 4 — relicsCarried increase → relicPickup cue", () => {
    it("emits a relicPickup cue when a ship's relicsCarried increases", () => {
      const prev = makeFrame(1, [makeShip("alpha", { relicsCarried: 0 })]);
      const curr = makeFrame(2, [makeShip("alpha", { relicsCarried: 1 })]);

      const cues = deriveSoundCues(prev, curr);
      const pickups = cues.filter((c) => c.kind === "relicPickup");
      expect(pickups).toHaveLength(1);
      expect(pickups[0]!.shipId).toBe("alpha");
    });

    it("does not emit a relicPickup cue when relicsCarried stays the same", () => {
      const prev = makeFrame(1, [makeShip("alpha", { relicsCarried: 2 })]);
      const curr = makeFrame(2, [makeShip("alpha", { relicsCarried: 2 })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "relicPickup")).toHaveLength(0);
    });

    it("does not emit a relicPickup cue when relicsCarried decreases", () => {
      const prev = makeFrame(1, [makeShip("alpha", { relicsCarried: 3 })]);
      const curr = makeFrame(2, [makeShip("alpha", { relicsCarried: 2 })]);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "relicPickup")).toHaveLength(0);
    });
  });

  // ── slice 5: relicBank cues ─────────────────────────────────────────────────

  describe("slice 5 — relicsCarried drop + score increase → relicBank cue", () => {
    it("emits a relicBank cue when relics drop and score increases", () => {
      const prev = makeFrame(
        1,
        [makeShip("alpha", { relicsCarried: 2 })],
        { scores: { alpha: 10 } },
      );
      const curr = makeFrame(
        2,
        [makeShip("alpha", { relicsCarried: 0 })],
        { scores: { alpha: 12 } },
      );

      const cues = deriveSoundCues(prev, curr);
      const banks = cues.filter((c) => c.kind === "relicBank");
      expect(banks).toHaveLength(1);
      expect(banks[0]!.shipId).toBe("alpha");
    });

    it("does NOT emit a relicBank cue when relics drop but score does NOT increase", () => {
      // Relics dropped due to death — no bank
      const prev = makeFrame(
        1,
        [makeShip("alpha", { alive: true, relicsCarried: 2 })],
        { scores: { alpha: 10 } },
      );
      const curr = makeFrame(
        2,
        [makeShip("alpha", { alive: false, relicsCarried: 0 })],
        { scores: { alpha: 10 } },
      );

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "relicBank")).toHaveLength(0);
    });

    it("does NOT emit a relicBank cue when score increases but relics stayed same", () => {
      const prev = makeFrame(1, [makeShip("alpha", { relicsCarried: 1 })], { scores: { alpha: 5 } });
      const curr = makeFrame(2, [makeShip("alpha", { relicsCarried: 1 })], { scores: { alpha: 7 } });

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "relicBank")).toHaveLength(0);
    });
  });

  // ── slice 6: matchStart cue ─────────────────────────────────────────────────

  describe("slice 6 — tick === 0 → matchStart cue", () => {
    it("emits a matchStart cue when prevFrame is null and curr.tick === 0", () => {
      const curr = makeFrame(0);

      const cues = deriveSoundCues(null, curr);
      expect(cues.filter((c) => c.kind === "matchStart")).toHaveLength(1);
    });

    it("emits a matchStart cue when curr.tick === 0 and prev.tick was non-zero", () => {
      const prev = makeFrame(99);
      const curr = makeFrame(0);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "matchStart")).toHaveLength(1);
    });

    it("does NOT emit a matchStart cue when both prev.tick and curr.tick are 0", () => {
      const prev = makeFrame(0);
      const curr = makeFrame(0);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "matchStart")).toHaveLength(0);
    });

    it("does NOT emit a matchStart cue mid-match", () => {
      const prev = makeFrame(5);
      const curr = makeFrame(6);

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "matchStart")).toHaveLength(0);
    });
  });

  // ── slice 7: matchEnd cue ───────────────────────────────────────────────────

  describe("slice 7 — tick crossing maxTicks → matchEnd cue (once)", () => {
    it("emits a matchEnd cue when curr.tick >= maxTicks and prev.tick < maxTicks", () => {
      const prev = makeFrame(99, [], { maxTicks: 100 });
      const curr = makeFrame(100, [], { maxTicks: 100 });

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "matchEnd")).toHaveLength(1);
    });

    it("does NOT re-emit matchEnd when both frames are at or past maxTicks", () => {
      const prev = makeFrame(100, [], { maxTicks: 100 });
      const curr = makeFrame(101, [], { maxTicks: 100 });

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "matchEnd")).toHaveLength(0);
    });

    it("emits matchEnd when prevFrame is null and curr.tick >= maxTicks", () => {
      // First frame received is already at max (edge: replay seeked to end)
      const curr = makeFrame(100, [], { maxTicks: 100 });

      const cues = deriveSoundCues(null, curr);
      expect(cues.filter((c) => c.kind === "matchEnd")).toHaveLength(1);
    });

    it("does NOT emit matchEnd mid-match", () => {
      const prev = makeFrame(40, [], { maxTicks: 100 });
      const curr = makeFrame(41, [], { maxTicks: 100 });

      const cues = deriveSoundCues(prev, curr);
      expect(cues.filter((c) => c.kind === "matchEnd")).toHaveLength(0);
    });
  });

  // ── slice 8: quiet frame → no cues ─────────────────────────────────────────

  describe("slice 8 — quiet frame with no deltas → no cues", () => {
    it("returns an empty array when nothing changed between frames", () => {
      const ship = makeShip("alpha", {
        alive: true,
        sigil: null,
        cannonCooldown: 0,
        relicsCarried: 0,
      });
      const prev = makeFrame(5, [ship], { scores: { alpha: 0 } });
      const curr = makeFrame(6, [ship], { scores: { alpha: 0 } });

      const cues = deriveSoundCues(prev, curr);
      // Filter out thrust cues (ship is stationary, so none expected here)
      const nonThrust = cues.filter((c) => c.kind !== "thrust");
      expect(nonThrust).toHaveLength(0);
    });

    it("returns an empty array when both frames are empty mid-match", () => {
      const prev = makeFrame(5);
      const curr = makeFrame(6);

      const cues = deriveSoundCues(prev, curr);
      expect(cues).toHaveLength(0);
    });
  });

  // ── thrust cues ────────────────────────────────────────────────────────────

  describe("thrust cues — living thrusting ships emit thrust cues", () => {
    it("emits a thrust cue for a ship with afterburnerTicksLeft > 0", () => {
      const curr = makeFrame(5, [makeShip("alpha", { afterburnerTicksLeft: 3 })]);

      const cues = deriveSoundCues(null, curr);
      const thrustCues = cues.filter((c) => c.kind === "thrust");
      expect(thrustCues).toHaveLength(1);
      expect(thrustCues[0]!.shipId).toBe("alpha");
    });

    it("does not emit a thrust cue for a dead ship", () => {
      const curr = makeFrame(5, [
        makeShip("alpha", { alive: false, afterburnerTicksLeft: 3 }),
      ]);

      const cues = deriveSoundCues(null, curr);
      expect(cues.filter((c) => c.kind === "thrust")).toHaveLength(0);
    });

    it("does not emit a thrust cue for a stationary ship", () => {
      const curr = makeFrame(5, [
        makeShip("alpha", { vel: { x: 0, y: 0 }, afterburnerTicksLeft: 0 }),
      ]);

      const cues = deriveSoundCues(null, curr);
      expect(cues.filter((c) => c.kind === "thrust")).toHaveLength(0);
    });
  });
});
