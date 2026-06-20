/**
 * Viewer entry point — wires the ObserverClient and DriftRenderer together,
 * and attaches camera input handlers (zoom/pan/follow).
 *
 * On load:
 *   1. Initialises the PixiJS renderer in #viewer-canvas-container.
 *   2. Opens a WebSocket connection to the Arena's /observe stream.
 *   3. Calls renderer.renderFrame(frame) on each parsed god-view tick.
 *   4. Wires keyboard + mouse input to renderer.camera for zoom/pan/follow.
 *
 * Camera controls (manual / visual — not unit-tested per PRD):
 *   Scroll wheel   — zoom in/out, centred on cursor position
 *   +/= key        — zoom in  (×1.2)
 *   -/_ key        — zoom out (×1/1.2)
 *   Arrow keys     — pan (20 px/step)
 *   Click on ship  — follow that ship (centres camera on it)
 *   F / Escape     — reset to fit-the-Drift (default)
 *   R              — reset pan to zero (keep zoom/mode)
 *
 * Seams for future issues:
 *   03  — ship team colours / labels / bars come from the renderer
 *   04  — Sigil effects / explosions come from the renderer
 *   05  — HUD overlay wired here; read renderer.camera.followTarget for highlight
 *   06  — sound module wired here (receives same frame)
 *   07  — replay loader feeds the same renderFrame callback, bypassing wsClient
 */

import { DriftRenderer } from "./renderer.ts";
import { ObserverClient, defaultObserverUrl } from "./wsClient.ts";
import { screenToWorld, worldToScreen } from "../lib/worldTransform.ts";
import type { GodViewFrame } from "../lib/frameParser.ts";
import type { Vec2 } from "../lib/worldTransform.ts";

// ── Follow-select threshold ───────────────────────────────────────────────────

/** Maximum screen-pixel distance from a click to count as selecting a ship. */
const FOLLOW_PICK_RADIUS_PX = 20;

// ── Input wiring ──────────────────────────────────────────────────────────────

/**
 * Attach all camera input handlers to the PixiJS canvas element.
 * Called once after the renderer is created.
 *
 * @param canvas  The HTMLCanvasElement owned by PixiJS.
 * @param renderer  The DriftRenderer whose `.camera` is mutated.
 * @param getLastFrame  Accessor for the most-recently-received frame
 *                      (needed to resolve canvas size + ship list for picking).
 */
function wireCameraInput(
  canvas: HTMLCanvasElement,
  renderer: DriftRenderer,
  getLastFrame: () => GodViewFrame | null,
): void {
  const cam = renderer.camera;

  // ── Scroll wheel: zoom centred on cursor ─────────────────────────────────
  canvas.addEventListener("wheel", (e: WheelEvent) => {
    e.preventDefault();
    const frame = getLastFrame();
    if (!frame) return;

    const rect = canvas.getBoundingClientRect();
    const cursorScreen: Vec2 = { x: e.clientX - rect.left, y: e.clientY - rect.top };
    const canvasSize = { width: renderer.canvasWidth, height: renderer.canvasHeight };

    // World point under cursor before zoom
    const oldT = cam.getTransform(frame.arena, canvasSize, undefined);
    const worldAtCursor = screenToWorld(cursorScreen, oldT);

    // Apply zoom
    const factor = e.deltaY < 0 ? 1.2 : 1 / 1.2;
    cam.zoomBy(factor);

    // Adjust pan so the same world point stays under the cursor
    const newT = cam.getTransform(frame.arena, canvasSize, undefined);
    const newScreenAtCursor = worldToScreen(worldAtCursor, newT);
    cam.pan(cursorScreen.x - newScreenAtCursor.x, cursorScreen.y - newScreenAtCursor.y);
  }, { passive: false });

  // ── Pointer drag: pan ─────────────────────────────────────────────────────
  let dragStart: { x: number; y: number } | null = null;

  canvas.addEventListener("pointerdown", (e: PointerEvent) => {
    if (e.button === 1 || e.button === 2) {
      // Middle or right mouse button starts a pan drag
      dragStart = { x: e.clientX, y: e.clientY };
      canvas.setPointerCapture(e.pointerId);
      e.preventDefault();
    }
  });

  canvas.addEventListener("pointermove", (e: PointerEvent) => {
    if (dragStart === null) return;
    cam.pan(e.clientX - dragStart.x, e.clientY - dragStart.y);
    dragStart = { x: e.clientX, y: e.clientY };
  });

  canvas.addEventListener("pointerup", (e: PointerEvent) => {
    if (dragStart !== null) {
      dragStart = null;
      canvas.releasePointerCapture(e.pointerId);
    }
  });

  // ── Left click: follow nearest ship ──────────────────────────────────────
  canvas.addEventListener("click", (e: MouseEvent) => {
    // Ignore if it looks like the end of a drag
    if (e.button !== 0) return;
    const frame = getLastFrame();
    if (!frame) return;

    const rect = canvas.getBoundingClientRect();
    const clickScreen: Vec2 = { x: e.clientX - rect.left, y: e.clientY - rect.top };
    const canvasSize = { width: renderer.canvasWidth, height: renderer.canvasHeight };

    const followTarget = cam.followTarget;
    const t = cam.getTransform(frame.arena, canvasSize,
      followTarget ? frame.ships.find(s => s.id === followTarget)?.pos : undefined);
    const clickWorld = screenToWorld(clickScreen, t);

    // Find closest alive ship within the pick radius
    let closest: string | null = null;
    let bestDist = FOLLOW_PICK_RADIUS_PX / t.scale; // convert px threshold to world units

    for (const ship of frame.ships) {
      if (!ship.alive) continue;
      const dx = ship.pos.x - clickWorld.x;
      const dy = ship.pos.y - clickWorld.y;
      const d = Math.sqrt(dx * dx + dy * dy);
      if (d < bestDist) {
        bestDist = d;
        closest = ship.id;
      }
    }

    if (closest !== null) {
      cam.follow(closest);
    }
  });

  // ── Keyboard ──────────────────────────────────────────────────────────────
  const PAN_STEP = 20; // pixels per arrow-key press

  window.addEventListener("keydown", (e: KeyboardEvent) => {
    // Don't steal keys from form inputs
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

    switch (e.key) {
      case "=": case "+":
        cam.zoomBy(1.2);
        break;
      case "-": case "_":
        cam.zoomBy(1 / 1.2);
        break;
      case "ArrowLeft":
        cam.pan(-PAN_STEP, 0);
        e.preventDefault();
        break;
      case "ArrowRight":
        cam.pan(PAN_STEP, 0);
        e.preventDefault();
        break;
      case "ArrowUp":
        cam.pan(0, -PAN_STEP);
        e.preventDefault();
        break;
      case "ArrowDown":
        cam.pan(0, PAN_STEP);
        e.preventDefault();
        break;
      case "f": case "F": case "Escape":
        // Reset to fit-the-Drift
        cam.setMode("fit").resetPan().setZoom(1);
        break;
      case "r": case "R":
        // Reset pan only, preserve zoom/mode
        cam.resetPan();
        break;
    }
  });

  // Suppress browser context menu on the canvas (right-click used for drag)
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
}

// ── Entry point ───────────────────────────────────────────────────────────────

async function init(): Promise<void> {
  const statusBar = document.getElementById("status-bar");
  const container = document.getElementById("viewer-canvas-container");

  if (!container) {
    console.error("[Viewer] #viewer-canvas-container not found");
    return;
  }

  const setStatus = (msg: string): void => {
    if (statusBar) statusBar.textContent = `⬡ ${msg}`;
  };

  setStatus("initialising renderer…");

  let renderer: DriftRenderer;
  try {
    renderer = await DriftRenderer.create(container);
  } catch (err) {
    setStatus(`renderer init failed: ${String(err)}`);
    console.error("[Viewer] renderer init failed", err);
    return;
  }

  // Track the latest frame for cursor-relative zoom and click-to-follow
  let lastFrame: GodViewFrame | null = null;

  // Wire camera input after renderer (and thus the PixiJS canvas) exists
  const pixiCanvas = container.querySelector("canvas");
  if (pixiCanvas instanceof HTMLCanvasElement) {
    wireCameraInput(pixiCanvas, renderer, () => lastFrame);
  }

  const wsUrl = defaultObserverUrl();
  setStatus(`connecting to ${wsUrl}…`);

  const client = new ObserverClient({
    url: wsUrl,
    onFrame(frame) {
      lastFrame = frame;
      renderer.renderFrame(frame);
    },
    onStatus: setStatus,
  });

  client.connect();
}

init().catch(console.error);

