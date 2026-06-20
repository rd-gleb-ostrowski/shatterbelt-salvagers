import { describe, it, expect } from "vitest";
import {
  fitDriftTransform,
  worldToScreen,
  screenToWorld,
} from "./worldTransform.ts";

// ── fitDriftTransform ─────────────────────────────────────────────────────────

describe("fitDriftTransform", () => {
  it("square arena on square canvas: scale fills full canvas, no offset", () => {
    const t = fitDriftTransform({ width: 1000, height: 1000 }, { width: 800, height: 800 });
    expect(t.scale).toBeCloseTo(0.8);
    expect(t.offsetX).toBeCloseTo(0);
    expect(t.offsetY).toBeCloseTo(0);
  });

  it("wide arena on square canvas: letterboxed top/bottom", () => {
    // 2000×1000 arena on 800×800 canvas → scale limited by width: 800/2000=0.4
    // rendered height = 1000*0.4 = 400 → offsetY = (800-400)/2 = 200
    const t = fitDriftTransform({ width: 2000, height: 1000 }, { width: 800, height: 800 });
    expect(t.scale).toBeCloseTo(0.4);
    expect(t.offsetX).toBeCloseTo(0);
    expect(t.offsetY).toBeCloseTo(200);
  });

  it("tall arena on square canvas: pillarboxed left/right", () => {
    // 1000×2000 arena on 800×800 canvas → scale limited by height: 800/2000=0.4
    // rendered width = 1000*0.4 = 400 → offsetX = (800-400)/2 = 200
    const t = fitDriftTransform({ width: 1000, height: 2000 }, { width: 800, height: 800 });
    expect(t.scale).toBeCloseTo(0.4);
    expect(t.offsetX).toBeCloseTo(200);
    expect(t.offsetY).toBeCloseTo(0);
  });

  it("arena on wider canvas: pillarboxed", () => {
    // 1000×1000 arena on 1600×800 canvas → scale limited by height: 800/1000=0.8
    // rendered width = 800 → offsetX = (1600-800)/2 = 400
    const t = fitDriftTransform({ width: 1000, height: 1000 }, { width: 1600, height: 800 });
    expect(t.scale).toBeCloseTo(0.8);
    expect(t.offsetX).toBeCloseTo(400);
    expect(t.offsetY).toBeCloseTo(0);
  });
});

// ── worldToScreen ─────────────────────────────────────────────────────────────

describe("worldToScreen", () => {
  it("world origin maps to the offset (top-left of Drift on canvas)", () => {
    const t = fitDriftTransform({ width: 1000, height: 1000 }, { width: 800, height: 800 });
    const screen = worldToScreen({ x: 0, y: 0 }, t);
    expect(screen.x).toBeCloseTo(0);
    expect(screen.y).toBeCloseTo(0);
  });

  it("world centre maps to canvas centre when Drift fills the canvas", () => {
    const t = fitDriftTransform({ width: 1000, height: 1000 }, { width: 800, height: 800 });
    const screen = worldToScreen({ x: 500, y: 500 }, t);
    expect(screen.x).toBeCloseTo(400);
    expect(screen.y).toBeCloseTo(400);
  });

  it("world origin maps inside letterbox offset", () => {
    // Wide arena: 2000×1000 on 800×800 → offsetY=200, scale=0.4
    const t = fitDriftTransform({ width: 2000, height: 1000 }, { width: 800, height: 800 });
    const screen = worldToScreen({ x: 0, y: 0 }, t);
    expect(screen.x).toBeCloseTo(0);
    expect(screen.y).toBeCloseTo(200);
  });

  it("world bottom-right corner maps to canvas edge of rendered region", () => {
    // Wide arena: 2000×1000 on 800×800 → scale=0.4, offsetY=200
    // (2000,1000) → x=800, y=400+200=600
    const t = fitDriftTransform({ width: 2000, height: 1000 }, { width: 800, height: 800 });
    const screen = worldToScreen({ x: 2000, y: 1000 }, t);
    expect(screen.x).toBeCloseTo(800);
    expect(screen.y).toBeCloseTo(600);
  });
});

// ── screenToWorld (roundtrip) ─────────────────────────────────────────────────

describe("screenToWorld", () => {
  it("round-trips worldToScreen for an arbitrary point", () => {
    const arena = { width: 1500, height: 900 };
    const canvas = { width: 1920, height: 1080 };
    const t = fitDriftTransform(arena, canvas);
    const world = { x: 742.5, y: 300 };
    const back = screenToWorld(worldToScreen(world, t), t);
    expect(back.x).toBeCloseTo(world.x, 4);
    expect(back.y).toBeCloseTo(world.y, 4);
  });
});
