/**
 * camera — framework-free mutable camera model for the Drift Viewer.
 *
 * Modes:
 *   "fit"    — centres the entire Drift inside the canvas (default). The
 *              transform is recomputed each call to getTransform() so changing
 *              arena sizes (different matches) are handled automatically.
 *   "follow" — centres the canvas on a chosen ship's world position, tracking
 *              it across frames.
 *
 * Zoom and pan compose on top of either mode.
 *
 * The single public output is `getTransform(arena, canvas, shipPos?)`, which
 * returns the CameraTransform consumed by DriftRenderer / worldToScreen.
 *
 * Pure compute — no DOM, no PixiJS.  Covered by unit tests in camera.test.ts.
 * Seam for issue 03+: expose `camera.followTarget` so HUD / ship-cues can
 * read which ship is tracked without touching this module.
 */

import type { ArenaDims, CanvasSize, CameraTransform, Vec2 } from "./worldTransform.ts";
import { fitDriftTransform } from "./worldTransform.ts";

// ── Constants ─────────────────────────────────────────────────────────────────

export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 20;

// ── Types ─────────────────────────────────────────────────────────────────────

export type CameraMode = "fit" | "follow";

/** Snapshot of the full camera state — returned by getState(). */
export interface CameraState {
  mode: CameraMode;
  zoom: number;
  panOffset: Vec2;
  followTarget: string | null;
}

// ── Camera ────────────────────────────────────────────────────────────────────

/**
 * Mutable camera model.  Instantiate once; update in response to user input;
 * call `getTransform` each frame to obtain the CameraTransform for the renderer.
 */
export class Camera {
  private _mode: CameraMode = "fit";
  private _zoom = 1;
  private _panOffset: Vec2 = { x: 0, y: 0 };
  private _followTarget: string | null = null;

  // ── Accessors ───────────────────────────────────────────────────────────────

  get mode(): CameraMode {
    return this._mode;
  }

  get zoom(): number {
    return this._zoom;
  }

  get panOffset(): Vec2 {
    return { ...this._panOffset };
  }

  get followTarget(): string | null {
    return this._followTarget;
  }

  // ── Mutators (chainable) ────────────────────────────────────────────────────

  /** Switch mode.  Switching to "fit" also clears followTarget. */
  setMode(mode: CameraMode): this {
    this._mode = mode;
    if (mode === "fit") this._followTarget = null;
    return this;
  }

  /**
   * Switch to follow mode and track the given ship ID.
   * Seam for issue 03+ / HUD: read `camera.followTarget` to know which ship
   * is being tracked without modifying this module.
   */
  follow(shipId: string): this {
    this._mode = "follow";
    this._followTarget = shipId;
    return this;
  }

  /** Set the zoom level, clamped to [MIN_ZOOM, MAX_ZOOM]. */
  setZoom(zoom: number): this {
    this._zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, zoom));
    return this;
  }

  /** Multiply the current zoom by `factor` (result is clamped). */
  zoomBy(factor: number): this {
    return this.setZoom(this._zoom * factor);
  }

  /** Set the pan offset in screen pixels (replaces current offset). */
  setPanOffset(offset: Vec2): this {
    this._panOffset = { ...offset };
    return this;
  }

  /** Accumulate a pan delta in screen pixels. */
  pan(dx: number, dy: number): this {
    this._panOffset = { x: this._panOffset.x + dx, y: this._panOffset.y + dy };
    return this;
  }

  /** Reset pan offset to zero. */
  resetPan(): this {
    this._panOffset = { x: 0, y: 0 };
    return this;
  }

  // ── Transform ───────────────────────────────────────────────────────────────

  /**
   * Compute the CameraTransform for the current frame.
   *
   * Math:
   *   effectiveScale = fitScale * zoom
   *
   *   fit mode:    offsetX = (canvasW - arenaW * effectiveScale) / 2  + panX
   *   follow mode: offsetX = canvasW/2 - shipX * effectiveScale        + panX
   *
   * The fit formula centres the arena and zooms around the canvas centre.
   * The follow formula places the followed ship's world position at the canvas
   * centre, then applies pan on top.
   *
   * @param arena    Arena dimensions from the current frame.
   * @param canvas   Rendered canvas pixel dimensions.
   * @param shipPos  World position of the followed ship.  Required when mode
   *                 is "follow"; ignored in "fit" mode.
   */
  getTransform(arena: ArenaDims, canvas: CanvasSize, shipPos?: Vec2): CameraTransform {
    const fit = fitDriftTransform(arena, canvas);
    const scale = fit.scale * this._zoom;

    let offsetX: number;
    let offsetY: number;

    if (this._mode === "follow" && shipPos !== undefined) {
      offsetX = canvas.width / 2 - shipPos.x * scale;
      offsetY = canvas.height / 2 - shipPos.y * scale;
    } else {
      offsetX = (canvas.width - arena.width * scale) / 2;
      offsetY = (canvas.height - arena.height * scale) / 2;
    }

    return {
      scale,
      offsetX: offsetX + this._panOffset.x,
      offsetY: offsetY + this._panOffset.y,
    };
  }

  /** Return the full camera state as a plain snapshot. */
  getState(): CameraState {
    return {
      mode: this._mode,
      zoom: this._zoom,
      panOffset: { ...this._panOffset },
      followTarget: this._followTarget,
    };
  }
}
