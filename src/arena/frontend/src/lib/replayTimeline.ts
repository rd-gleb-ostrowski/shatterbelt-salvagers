/**
 * replayTimeline — pure, framework-free timeline math for the replay player.
 *
 * Two concerns:
 *   (a) Scrub mapping: normalized timeline position [0,1] ↔ frame index.
 *   (b) Playback advance: given accumulated playback time, speed, and tick rate,
 *       compute the current frame index to display.
 *
 * The server runs at 30 ticks/s.  At speed=1 the replay consumes exactly one
 * frame per tick-duration (33.33 ms); at speed=2 it consumes two; at speed=0.5
 * it advances one frame every two tick-durations.
 *
 * Design notes:
 *   - All functions are pure (no side effects, no global state).
 *   - The ReplayPlayer tracks `playbackMs` as a float (accumulated real-time ×
 *     speed) and calls `playbackMsToIndex` each RAF step to avoid sub-frame
 *     drift.  `advanceIndex` is the equivalent expressed as a delta (used in
 *     unit tests).
 *   - Scrub <-> index roundtrip is exact at integer indices (see tests).
 *
 * Seam for issue 07: consumed by ReplayPlayer (viewer/replayPlayer.ts).
 */

// ── Constants ─────────────────────────────────────────────────────────────────

/** Server tick rate in ticks per second. */
export const TICK_RATE = 30;

/** Nominal real-time duration of one tick in milliseconds. */
export const TICK_DURATION_MS = 1000 / TICK_RATE;

// ── Scrub position ↔ index mapping ────────────────────────────────────────────

/**
 * Convert a normalized scrub position [0,1] to a frame buffer index.
 *
 * - position=0   → first frame (index 0)
 * - position=1   → last frame  (index bufferLength-1)
 * - Clamps: values <0 map to 0; values >1 map to bufferLength-1.
 *
 * @param bufferLength  Total number of frames in the buffer (must be ≥1).
 * @param position      Normalized scrub position in [0,1].
 * @returns             Integer frame index in [0, bufferLength-1].
 *
 * Pure function — deterministic, no I/O.
 */
export function positionToIndex(bufferLength: number, position: number): number {
  if (bufferLength <= 1) return 0;
  const clamped = Math.max(0, Math.min(1, position));
  return Math.round(clamped * (bufferLength - 1));
}

/**
 * Convert a frame buffer index to a normalized scrub position [0,1].
 *
 * Inverse of `positionToIndex`.  Integer indices round-trip exactly.
 *
 * @param bufferLength  Total number of frames in the buffer (must be ≥1).
 * @param index         Frame index (clamped to [0, bufferLength-1]).
 * @returns             Normalized scrub position in [0,1].
 *
 * Pure function — deterministic, no I/O.
 */
export function indexToPosition(bufferLength: number, index: number): number {
  if (bufferLength <= 1) return 0;
  const clamped = Math.max(0, Math.min(bufferLength - 1, index));
  return clamped / (bufferLength - 1);
}

// ── Playback advance ──────────────────────────────────────────────────────────

/**
 * Compute the current frame index from accumulated playback time.
 *
 * This is the canonical advance calculation used by ReplayPlayer each RAF
 * step: the player maintains a float `playbackMs` (accumulated real-time ×
 * speed) and calls this function to get the display index.  Accumulating
 * continuously before flooring avoids sub-frame drift in incremental calls.
 *
 * @param playbackMs   Total accumulated playback time in ms (real time × speed).
 * @param bufferLength Total number of frames in the buffer.
 * @returns            Integer frame index in [0, bufferLength-1].
 *
 * Pure function — deterministic, no I/O.
 */
export function playbackMsToIndex(playbackMs: number, bufferLength: number): number {
  if (bufferLength <= 1) return 0;
  const raw = Math.floor((playbackMs / 1000) * TICK_RATE);
  return Math.min(raw, bufferLength - 1);
}

/**
 * Advance a frame index by a real-time delta at the given speed multiplier.
 *
 * Equivalent to `playbackMsToIndex` called with the delta since the current
 * index's origin.  Use this for single-step calculations (tests); for a
 * running playback loop prefer tracking `playbackMs` as a float and calling
 * `playbackMsToIndex` to avoid sub-frame drift.
 *
 * At speed=1, one tick-duration (33.33 ms) advances exactly one frame.
 * At speed=2, one tick-duration advances two frames.
 * At speed=0.5, two tick-durations advance one frame.
 *
 * Result is clamped to [currentIndex, bufferLength-1] (never goes backwards,
 * never overruns the last frame).
 *
 * @param currentIndex  Current frame index (integer).
 * @param elapsedMs     Real time elapsed since last advance (ms).
 * @param speed         Speed multiplier (e.g. 1, 2, 0.5).
 * @param bufferLength  Total number of frames in the buffer.
 * @returns             Next frame index in [currentIndex, bufferLength-1].
 *
 * Pure function — deterministic, no I/O.
 */
export function advanceIndex(
  currentIndex: number,
  elapsedMs: number,
  speed: number,
  bufferLength: number,
): number {
  if (bufferLength <= 1) return 0;
  const delta = Math.floor((elapsedMs * speed) / TICK_DURATION_MS);
  const next = currentIndex + delta;
  return Math.min(next, bufferLength - 1);
}
