import { describe, it, expect } from "vitest";
import { Camera, MIN_ZOOM, MAX_ZOOM } from "./camera.ts";
import { fitDriftTransform, worldToScreen } from "./worldTransform.ts";

// ── Slice 1: fit mode with zoom=1 reproduces fitDriftTransform ────────────────

describe("Camera.getTransform — fit mode (default)", () => {
  it("default state produces the same transform as fitDriftTransform", () => {
    const arena = { width: 2000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const expected = fitDriftTransform(arena, canvas);
    const actual = new Camera().getTransform(arena, canvas);
    expect(actual.scale).toBeCloseTo(expected.scale);
    expect(actual.offsetX).toBeCloseTo(expected.offsetX);
    expect(actual.offsetY).toBeCloseTo(expected.offsetY);
  });

  it("recomputes correctly when arena dims change (wide then tall)", () => {
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    const t1 = cam.getTransform({ width: 2000, height: 1000 }, canvas);
    const t2 = cam.getTransform({ width: 1000, height: 2000 }, canvas);
    // Pillarbox: wide arena gives offsetY, tall arena gives offsetX
    expect(t1.offsetY).toBeCloseTo(200);
    expect(t2.offsetX).toBeCloseTo(200);
  });
});

// ── Slice 2: zoom-in scales world→screen mapping ──────────────────────────────

describe("Camera.getTransform — zoom", () => {
  it("zoom=2 doubles the scale relative to fit", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.setZoom(2);
    const t = cam.getTransform(arena, canvas);
    const fitScale = fitDriftTransform(arena, canvas).scale;
    expect(t.scale).toBeCloseTo(fitScale * 2);
  });

  it("zoom=0.5 halves the scale relative to fit", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.setZoom(0.5);
    const t = cam.getTransform(arena, canvas);
    const fitScale = fitDriftTransform(arena, canvas).scale;
    expect(t.scale).toBeCloseTo(fitScale * 0.5);
  });

  it("fit+zoom keeps the arena centre at the canvas centre", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.setZoom(2);
    const t = cam.getTransform(arena, canvas);
    const arenaCenter = { x: arena.width / 2, y: arena.height / 2 };
    const screenCenter = worldToScreen(arenaCenter, t);
    expect(screenCenter.x).toBeCloseTo(canvas.width / 2);
    expect(screenCenter.y).toBeCloseTo(canvas.height / 2);
  });

  it("zoomBy multiplies the current zoom", () => {
    const cam = new Camera();
    cam.setZoom(2);
    cam.zoomBy(1.5);
    expect(cam.zoom).toBeCloseTo(3);
  });
});

// ── Slice 3: pan shifts the mapping by a screen offset ────────────────────────

describe("Camera.getTransform — pan", () => {
  it("pan offset shifts world-to-screen by the given pixels", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.setPanOffset({ x: 50, y: -30 });
    const t = cam.getTransform(arena, canvas);
    const fitT = fitDriftTransform(arena, canvas);
    const screen = worldToScreen({ x: 0, y: 0 }, t);
    const fitScreen = worldToScreen({ x: 0, y: 0 }, fitT);
    expect(screen.x).toBeCloseTo(fitScreen.x + 50);
    expect(screen.y).toBeCloseTo(fitScreen.y - 30);
  });

  it("pan() accumulates deltas", () => {
    const cam = new Camera();
    cam.pan(20, 10);
    cam.pan(-5, 7);
    expect(cam.panOffset.x).toBeCloseTo(15);
    expect(cam.panOffset.y).toBeCloseTo(17);
  });

  it("resetPan clears the offset", () => {
    const cam = new Camera();
    cam.pan(100, 200);
    cam.resetPan();
    expect(cam.panOffset.x).toBeCloseTo(0);
    expect(cam.panOffset.y).toBeCloseTo(0);
  });
});

// ── Slice 4: follow mode centres the ship at canvas centre ────────────────────

describe("Camera.getTransform — follow mode", () => {
  it("follow mode centres the followed ship at the canvas centre", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.follow("ship-1");
    const shipPos = { x: 300, y: 700 };
    const t = cam.getTransform(arena, canvas, shipPos);
    const screenPos = worldToScreen(shipPos, t);
    expect(screenPos.x).toBeCloseTo(canvas.width / 2);
    expect(screenPos.y).toBeCloseTo(canvas.height / 2);
  });

  it("follow mode with a ship at arena centre behaves like fit", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.follow("ship-0");
    const arenaCenter = { x: 500, y: 500 };
    const t = cam.getTransform(arena, canvas, arenaCenter);
    const fitT = fitDriftTransform(arena, canvas);
    // When ship is at arena centre, follow produces same offsets as fit
    expect(t.offsetX).toBeCloseTo(fitT.offsetX);
    expect(t.offsetY).toBeCloseTo(fitT.offsetY);
  });
});

// ── Slice 5: follow + zoom compose ───────────────────────────────────────────

describe("Camera.getTransform — follow + zoom compose", () => {
  it("follow+zoom: the followed ship stays at canvas centre while zoomed", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.follow("ship-1").setZoom(3);
    const shipPos = { x: 200, y: 400 };
    const t = cam.getTransform(arena, canvas, shipPos);
    const screenPos = worldToScreen(shipPos, t);
    expect(screenPos.x).toBeCloseTo(canvas.width / 2);
    expect(screenPos.y).toBeCloseTo(canvas.height / 2);
  });

  it("follow+zoom+pan: ship offset from centre by pan", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.follow("ship-1").setZoom(2).setPanOffset({ x: 100, y: -50 });
    const shipPos = { x: 500, y: 500 };
    const t = cam.getTransform(arena, canvas, shipPos);
    const screenPos = worldToScreen(shipPos, t);
    // Ship is displaced from centre by the pan offset
    expect(screenPos.x).toBeCloseTo(canvas.width / 2 + 100);
    expect(screenPos.y).toBeCloseTo(canvas.height / 2 - 50);
  });
});

// ── Slice 6: clamping ─────────────────────────────────────────────────────────

describe("Camera — zoom clamping", () => {
  it("setZoom clamps to MIN_ZOOM when given a value below it", () => {
    const cam = new Camera();
    cam.setZoom(0);
    expect(cam.zoom).toBe(MIN_ZOOM);
  });

  it("setZoom clamps to MAX_ZOOM when given a value above it", () => {
    const cam = new Camera();
    cam.setZoom(9999);
    expect(cam.zoom).toBe(MAX_ZOOM);
  });

  it("zoomBy clamps correctly when compounding past the limit", () => {
    const cam = new Camera();
    cam.setZoom(MAX_ZOOM);
    cam.zoomBy(2); // would exceed max
    expect(cam.zoom).toBe(MAX_ZOOM);
  });
});

// ── Slice 7: switching follow target / back to fit ───────────────────────────

describe("Camera — mode switching", () => {
  it("switching follow target centres the new ship", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.follow("ship-1");
    cam.follow("ship-2"); // switch to different ship
    const ship2Pos = { x: 800, y: 100 };
    const t = cam.getTransform(arena, canvas, ship2Pos);
    const screenPos = worldToScreen(ship2Pos, t);
    expect(screenPos.x).toBeCloseTo(canvas.width / 2);
    expect(screenPos.y).toBeCloseTo(canvas.height / 2);
  });

  it("followTarget is updated when follow() is called", () => {
    const cam = new Camera();
    cam.follow("ship-1");
    expect(cam.followTarget).toBe("ship-1");
    cam.follow("ship-2");
    expect(cam.followTarget).toBe("ship-2");
  });

  it("setMode('fit') clears followTarget and returns fit transform", () => {
    const arena = { width: 1000, height: 1000 };
    const canvas = { width: 800, height: 800 };
    const cam = new Camera();
    cam.follow("ship-1").setMode("fit");
    expect(cam.followTarget).toBeNull();
    expect(cam.mode).toBe("fit");
    const t = cam.getTransform(arena, canvas, { x: 300, y: 400 });
    const fitT = fitDriftTransform(arena, canvas);
    expect(t.scale).toBeCloseTo(fitT.scale);
    expect(t.offsetX).toBeCloseTo(fitT.offsetX);
    expect(t.offsetY).toBeCloseTo(fitT.offsetY);
  });

  it("getState returns a snapshot of current camera state", () => {
    const cam = new Camera();
    cam.follow("ship-3").setZoom(1.5).setPanOffset({ x: 10, y: 20 });
    const state = cam.getState();
    expect(state.mode).toBe("follow");
    expect(state.followTarget).toBe("ship-3");
    expect(state.zoom).toBeCloseTo(1.5);
    expect(state.panOffset.x).toBeCloseTo(10);
    expect(state.panOffset.y).toBeCloseTo(20);
  });
});
