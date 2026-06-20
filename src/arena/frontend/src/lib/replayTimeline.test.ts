/**
 * replayTimeline.test.ts — unit tests for the pure replay timeline functions.
 *
 * Covers all tracer slices specified in the PRD / issue 07:
 *   (1) Scrub position 0 → first frame; position 1 → last frame; midpoint → middle
 *   (2) Clamp: position <0 → first frame; position >1 → last frame
 *   (3) Roundtrip: index → position → index is exact for integer indices
 *   (4) advance at 1x over one tick-duration moves exactly one frame
 *   (5) advance at 2x moves two frames; at 0.5x moves one frame every two tick-durations
 *   (6) advance clamps at the final frame (no overrun)
 *   (7) playbackMsToIndex accumulates without sub-frame drift
 *
 * All tests are pure input→output assertions; no DOM, no I/O.
 */

import { describe, it, expect } from "vitest";
import {
  TICK_RATE,
  TICK_DURATION_MS,
  positionToIndex,
  indexToPosition,
  advanceIndex,
  playbackMsToIndex,
} from "./replayTimeline.ts";

// ── Slice 1: scrub position → index ──────────────────────────────────────────

describe("positionToIndex — slice 1: position → index mapping", () => {
  it("position 0 maps to the first frame (index 0)", () => {
    expect(positionToIndex(100, 0)).toBe(0);
  });

  it("position 1 maps to the last frame (index bufferLength-1)", () => {
    expect(positionToIndex(100, 1)).toBe(99);
  });

  it("midpoint 0.5 maps to the middle frame", () => {
    // bufferLength=101: indices 0..100; 0.5 * 100 = 50
    expect(positionToIndex(101, 0.5)).toBe(50);
  });

  it("maps 0.5 to middle index for even bufferLength (rounds to nearest)", () => {
    // bufferLength=10: 0.5 * 9 = 4.5 → rounds to 5
    expect(positionToIndex(10, 0.5)).toBe(5);
  });

  it("single-frame buffer always returns 0 regardless of position", () => {
    expect(positionToIndex(1, 0)).toBe(0);
    expect(positionToIndex(1, 0.5)).toBe(0);
    expect(positionToIndex(1, 1)).toBe(0);
  });
});

// ── Slice 2: clamping ─────────────────────────────────────────────────────────

describe("positionToIndex — slice 2: clamping out-of-range positions", () => {
  it("position < 0 clamps to first frame", () => {
    expect(positionToIndex(10, -0.5)).toBe(0);
    expect(positionToIndex(10, -99)).toBe(0);
  });

  it("position > 1 clamps to last frame", () => {
    expect(positionToIndex(10, 1.5)).toBe(9);
    expect(positionToIndex(10, 99)).toBe(9);
  });
});

// ── Slice 3: index → position roundtrip ─────────────────────────────────────

describe("indexToPosition — slice 3: index → position mapping and roundtrip", () => {
  it("index 0 maps to position 0", () => {
    expect(indexToPosition(100, 0)).toBe(0);
  });

  it("last index maps to position 1", () => {
    expect(indexToPosition(100, 99)).toBe(1);
  });

  it("middle index maps to 0.5", () => {
    // bufferLength=101: index 50 → 50/100 = 0.5
    expect(indexToPosition(101, 50)).toBe(0.5);
  });

  it("index → position → index roundtrip is exact for every integer index", () => {
    const len = 11;
    for (let i = 0; i < len; i++) {
      const pos = indexToPosition(len, i);
      const recovered = positionToIndex(len, pos);
      expect(recovered).toBe(i);
    }
  });

  it("single-frame buffer returns position 0 for any index", () => {
    expect(indexToPosition(1, 0)).toBe(0);
  });

  it("index < 0 clamps to position 0", () => {
    expect(indexToPosition(10, -5)).toBe(0);
  });

  it("index beyond last clamps to position 1", () => {
    expect(indexToPosition(10, 100)).toBe(1);
  });
});

// ── Slice 4: advance at 1x moves exactly one frame per tick-duration ─────────

describe("advanceIndex — slice 4: 1× speed over one tick-duration", () => {
  it("at 1× speed, advancing exactly one tick-duration moves exactly one frame", () => {
    expect(advanceIndex(0, TICK_DURATION_MS, 1, 100)).toBe(1);
  });

  it("at 1× speed, advancing N tick-durations moves N frames", () => {
    expect(advanceIndex(0, 5 * TICK_DURATION_MS, 1, 100)).toBe(5);
  });

  it("at 1× speed, advancing from a non-zero index moves correctly", () => {
    expect(advanceIndex(10, TICK_DURATION_MS, 1, 100)).toBe(11);
  });
});

// ── Slice 5: speed multiplier ─────────────────────────────────────────────────

describe("advanceIndex — slice 5: speed multiplier", () => {
  it("at 2× speed, one tick-duration advances two frames", () => {
    expect(advanceIndex(0, TICK_DURATION_MS, 2, 100)).toBe(2);
  });

  it("at 2× speed, N tick-durations advance 2N frames", () => {
    expect(advanceIndex(0, 3 * TICK_DURATION_MS, 2, 100)).toBe(6);
  });

  it("at 0.5× speed, one tick-duration does not yet advance a frame", () => {
    // 0.5× means we need two tick-durations to advance one frame
    expect(advanceIndex(0, TICK_DURATION_MS, 0.5, 100)).toBe(0);
  });

  it("at 0.5× speed, two tick-durations advance exactly one frame", () => {
    expect(advanceIndex(0, 2 * TICK_DURATION_MS, 0.5, 100)).toBe(1);
  });

  it("at 0.25× speed, four tick-durations advance exactly one frame", () => {
    expect(advanceIndex(0, 4 * TICK_DURATION_MS, 0.25, 100)).toBe(1);
  });
});

// ── Slice 6: clamping at the final frame ──────────────────────────────────────

describe("advanceIndex — slice 6: clamp at final frame", () => {
  it("does not advance past the last frame", () => {
    // bufferLength=10; last index=9; advance 100 frames from index 0
    expect(advanceIndex(0, 100 * TICK_DURATION_MS, 1, 10)).toBe(9);
  });

  it("returns last frame index when already at the end", () => {
    expect(advanceIndex(9, TICK_DURATION_MS, 1, 10)).toBe(9);
  });

  it("clamps at high speed multiplier without overrun", () => {
    expect(advanceIndex(0, TICK_DURATION_MS, 1000, 10)).toBe(9);
  });
});

// ── Slice 7: playbackMsToIndex — no sub-frame drift ──────────────────────────

describe("playbackMsToIndex — slice 7: accumulated playback time → index", () => {
  it("returns 0 at playbackMs = 0", () => {
    expect(playbackMsToIndex(0, 100)).toBe(0);
  });

  it("returns correct index at exact tick boundaries", () => {
    const len = 100;
    // After exactly N ticks worth of playback time, index should be N
    for (let n = 0; n < len; n++) {
      expect(playbackMsToIndex(n * TICK_DURATION_MS, len)).toBe(n);
    }
  });

  it("is consistent with advanceIndex at 1× when given the same elapsed time", () => {
    // Both must agree when called with complete tick multiples
    const elapsed = 10 * TICK_DURATION_MS;
    expect(playbackMsToIndex(elapsed, 100)).toBe(advanceIndex(0, elapsed, 1, 100));
  });

  it("clamps to last frame at end of buffer", () => {
    expect(playbackMsToIndex(999_999, 10)).toBe(9);
  });

  it("does not drift: continuous accumulation outperforms incremental flooring", () => {
    // Demonstrate why the player accumulates playbackMs as a float instead of
    // calling advanceIndex per-step.  At 0.5× speed, the player applies
    // `playbackMs += rafDt * speed` before calling playbackMsToIndex.
    // If the speed-scaled increment is smaller than one tick-duration, per-step
    // flooring would stall; accumulating first then converting gives the
    // correct result.
    //
    // Here we step at exactly 2 × TICK_DURATION_MS intervals with 1× speed
    // so each step advances exactly 2 frames — demonstrating exact tracking.
    const bufferLength = 30;
    let accumulated = 0;

    // After 0 steps: index 0
    expect(playbackMsToIndex(accumulated, bufferLength)).toBe(0);

    // After 1 step of 2 tick-durations: index 2
    accumulated += 2 * TICK_DURATION_MS;
    expect(playbackMsToIndex(accumulated, bufferLength)).toBe(2);

    // After 2 steps: index 4
    accumulated += 2 * TICK_DURATION_MS;
    expect(playbackMsToIndex(accumulated, bufferLength)).toBe(4);

    // After 10 steps: index 20
    for (let i = 0; i < 8; i++) accumulated += 2 * TICK_DURATION_MS;
    expect(playbackMsToIndex(accumulated, bufferLength)).toBe(20);
  });

  it("TICK_RATE constant is 30", () => {
    expect(TICK_RATE).toBe(30);
  });

  it("TICK_DURATION_MS constant is 1000/30", () => {
    expect(TICK_DURATION_MS).toBeCloseTo(1000 / 30, 10);
  });
});
