/**
 * worldTransform — framework-free world↔screen coordinate mapping.
 *
 * Seam for issue 02 (camera): replace `fitDriftTransform` with fit/follow/
 * zoom/pan variants; `worldToScreen` / `screenToWorld` stay the same shape.
 */

export interface Vec2 {
  x: number;
  y: number;
}

export interface ArenaDims {
  width: number;
  height: number;
}

export interface CanvasSize {
  width: number;
  height: number;
}

/**
 * An affine camera transform: screen_pos = world_pos * scale + offset.
 * Issue 02 will add follow/zoom/pan by computing different transforms here.
 */
export interface CameraTransform {
  /** World-units to pixels. */
  scale: number;
  /** Pixel offset for the world origin on-screen (x). */
  offsetX: number;
  /** Pixel offset for the world origin on-screen (y). */
  offsetY: number;
}

/**
 * Default transform: fit the entire Drift inside the canvas, centred,
 * preserving aspect ratio.  World origin (0,0) maps to the top-left of
 * the letterboxed Drift region.
 *
 * Pure function — no side effects, no global state; covered by unit tests.
 */
export function fitDriftTransform(
  arena: ArenaDims,
  canvas: CanvasSize
): CameraTransform {
  const scale = Math.min(canvas.width / arena.width, canvas.height / arena.height);
  const offsetX = (canvas.width - arena.width * scale) / 2;
  const offsetY = (canvas.height - arena.height * scale) / 2;
  return { scale, offsetX, offsetY };
}

/**
 * Map a world-space position to screen pixels using the given transform.
 *
 * Pure function — no side effects, no global state; covered by unit tests.
 */
export function worldToScreen(worldPos: Vec2, transform: CameraTransform): Vec2 {
  return {
    x: worldPos.x * transform.scale + transform.offsetX,
    y: worldPos.y * transform.scale + transform.offsetY,
  };
}

/**
 * Inverse of `worldToScreen` — map screen pixels back to world-space.
 *
 * Pure function — no side effects, no global state; covered by unit tests.
 */
export function screenToWorld(screenPos: Vec2, transform: CameraTransform): Vec2 {
  return {
    x: (screenPos.x - transform.offsetX) / transform.scale,
    y: (screenPos.y - transform.offsetY) / transform.scale,
  };
}
