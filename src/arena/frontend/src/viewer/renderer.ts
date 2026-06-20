/**
 * renderer — PixiJS WebGL renderer for the Drift Viewer.
 *
 * Renders: Drift bounds, asteroids, ships (with heading indicator),
 * relics (glowing), Anchors (team home beacons), projectiles, singularities,
 * mines.
 *
 * Design:
 * - All entity graphics are drawn to a single Graphics object per tick
 *   (cleared + redrawn) for simplicity in this first slice.
 * - The world->screen transform is produced by the Camera model (camera.ts),
 *   which replaced the direct fitDriftTransform call from issue 01.
 *
 * Seams for future issues:
 *   03  — add ship team colours, hull/shield bars, name labels, thrust flames
 *   04  — add Sigil effects (singularity well, mine indicator, arc-lance beam, explosions)
 *   05  — add HUD overlay (scoreboard, tick timer); read camera.followTarget for highlight
 *   06  — read frame events for Web Audio triggers (sound module)
 */

import { Application, Graphics, Container } from "pixi.js";
import type { GodViewFrame } from "../lib/frameParser.ts";
import {
  worldToScreen,
  type CameraTransform,
} from "../lib/worldTransform.ts";
import { Camera } from "../lib/camera.ts";

// ── Palette ───────────────────────────────────────────────────────────────────

const PALETTE = {
  driftBg: 0x05040f,
  driftBorder: 0x2a2050,
  asteroid: 0x5a5070,
  asteroidStroke: 0x8070a0,
  ship: 0x8aeeff,
  shipDead: 0x334455,
  shipInvuln: 0xd0a0ff,
  shipHeading: 0xffffff,
  anchor: 0xffcc44,
  anchorGlow: 0xffaa00,
  relic: 0x44ffaa,
  relicGlow: 0x00ff88,
  projectile: 0xffd060,
  singularity: 0xcc44ff,
  mine: 0xff4444,
  mineFriendly: 0x44aaff,
} as const;

// ── DriftRenderer ─────────────────────────────────────────────────────────────

export class DriftRenderer {
  private app: Application;
  private driftGraphics: Graphics;
  private entityGraphics: Graphics;
  private glowGraphics: Graphics;

  /** Camera instance — main.ts wires input handlers against this object. */
  readonly camera: Camera = new Camera();

  private constructor(
    app: Application,
    driftGraphics: Graphics,
    entityGraphics: Graphics,
    glowGraphics: Graphics,
  ) {
    this.app = app;
    this.driftGraphics = driftGraphics;
    this.entityGraphics = entityGraphics;
    this.glowGraphics = glowGraphics;
  }

  /**
   * Create and initialise the PixiJS application, then mount it.
   * Must be awaited before calling `renderFrame`.
   */
  static async create(container: HTMLElement): Promise<DriftRenderer> {
    const app = new Application();

    await app.init({
      resizeTo: container,
      background: PALETTE.driftBg,
      antialias: true,
      preference: "webgl",
      autoDensity: true,
      resolution: window.devicePixelRatio ?? 1,
    });

    container.appendChild(app.canvas);

    // Layer order: drift bg, glows (additive), entities on top
    const worldLayer = new Container();
    const driftGraphics = new Graphics();
    const glowGraphics = new Graphics();
    const entityGraphics = new Graphics();

    worldLayer.addChild(driftGraphics);
    worldLayer.addChild(glowGraphics);
    worldLayer.addChild(entityGraphics);
    app.stage.addChild(worldLayer);

    return new DriftRenderer(
      app,
      driftGraphics,
      entityGraphics,
      glowGraphics,
    );
  }

  get canvasWidth(): number {
    return this.app.screen.width;
  }

  get canvasHeight(): number {
    return this.app.screen.height;
  }

  /**
   * Render one god-view frame using the current camera state.
   *
   * Resolves the followed ship's world position from the frame (when in follow
   * mode) and delegates transform computation to `this.camera.getTransform`.
   */
  renderFrame(frame: GodViewFrame): void {
    const canvas = { width: this.canvasWidth, height: this.canvasHeight };

    // Resolve ship position for follow mode
    const followTarget = this.camera.followTarget;
    const shipPos = followTarget !== null
      ? frame.ships.find(s => s.id === followTarget)?.pos
      : undefined;

    const transform = this.camera.getTransform(frame.arena, canvas, shipPos);
    this.drawDrift(frame, transform);
    this.drawGlows(frame, transform);
    this.drawEntities(frame, transform);
  }

  // ── Layer: Drift border ───────────────────────────────────────────────────

  private drawDrift(frame: GodViewFrame, t: CameraTransform): void {
    this.driftGraphics.clear();
    const tl = worldToScreen({ x: 0, y: 0 }, t);
    const br = worldToScreen({ x: frame.arena.width, y: frame.arena.height }, t);
    const w = br.x - tl.x;
    const h = br.y - tl.y;

    // Subtle drift-border glow
    this.driftGraphics
      .rect(tl.x, tl.y, w, h)
      .stroke({ width: 2, color: PALETTE.driftBorder, alpha: 0.8 });
  }

  // ── Layer: Glows (rendered before entities for additive feel) ────────────

  private drawGlows(frame: GodViewFrame, t: CameraTransform): void {
    this.glowGraphics.clear();

    // Relic glow halos
    for (const relic of frame.relics) {
      const sp = worldToScreen(relic.pos, t);
      this.glowGraphics
        .circle(sp.x, sp.y, 18)
        .fill({ color: PALETTE.relicGlow, alpha: 0.18 });
      this.glowGraphics
        .circle(sp.x, sp.y, 10)
        .fill({ color: PALETTE.relicGlow, alpha: 0.28 });
    }

    // Anchor beacon glow rings
    for (const anchor of frame.anchors) {
      const sp = worldToScreen(anchor.pos, t);
      this.glowGraphics
        .circle(sp.x, sp.y, 22)
        .fill({ color: PALETTE.anchorGlow, alpha: 0.15 });
      this.glowGraphics
        .circle(sp.x, sp.y, 14)
        .fill({ color: PALETTE.anchorGlow, alpha: 0.22 });
    }

    // Singularity glow
    for (const sing of frame.singularities) {
      const sp = worldToScreen(sing.pos, t);
      const r = sing.radius * t.scale;
      this.glowGraphics
        .circle(sp.x, sp.y, r)
        .fill({ color: PALETTE.singularity, alpha: 0.12 });
      this.glowGraphics
        .circle(sp.x, sp.y, r * 0.5)
        .fill({ color: PALETTE.singularity, alpha: 0.25 });
    }
  }

  // ── Layer: Entities ───────────────────────────────────────────────────────

  private drawEntities(frame: GodViewFrame, t: CameraTransform): void {
    this.entityGraphics.clear();
    this.drawAsteroids(frame, t);
    this.drawRelics(frame, t);
    this.drawAnchors(frame, t);
    this.drawMines(frame, t);
    this.drawProjectiles(frame, t);
    this.drawSingularities(frame, t);
    this.drawShips(frame, t);
  }

  private drawAsteroids(frame: GodViewFrame, t: CameraTransform): void {
    for (const ast of frame.asteroids) {
      const sp = worldToScreen(ast.pos, t);
      const r = Math.max(ast.radius * t.scale, 3);
      this.entityGraphics
        .circle(sp.x, sp.y, r)
        .fill({ color: PALETTE.asteroid, alpha: 0.9 })
        .stroke({ width: 1, color: PALETTE.asteroidStroke, alpha: 0.7 });
    }
  }

  private drawRelics(frame: GodViewFrame, t: CameraTransform): void {
    for (const relic of frame.relics) {
      const sp = worldToScreen(relic.pos, t);
      // Diamond shape for relics
      const s = 6;
      this.entityGraphics
        .poly([sp.x, sp.y - s, sp.x + s, sp.y, sp.x, sp.y + s, sp.x - s, sp.y])
        .fill({ color: PALETTE.relic })
        .stroke({ width: 1, color: PALETTE.relicGlow, alpha: 0.9 });
    }
  }

  private drawAnchors(frame: GodViewFrame, t: CameraTransform): void {
    for (const anchor of frame.anchors) {
      const sp = worldToScreen(anchor.pos, t);
      // Cross/beacon shape
      const s = 10;
      this.entityGraphics
        .rect(sp.x - 2, sp.y - s, 4, s * 2)
        .fill({ color: PALETTE.anchor })
        .rect(sp.x - s, sp.y - 2, s * 2, 4)
        .fill({ color: PALETTE.anchor });
      // Outer ring
      this.entityGraphics
        .circle(sp.x, sp.y, 14)
        .stroke({ width: 2, color: PALETTE.anchor, alpha: 0.85 });
    }
  }

  private drawMines(frame: GodViewFrame, t: CameraTransform): void {
    for (const mine of frame.mines) {
      const sp = worldToScreen(mine.pos, t);
      const color = mine.own ? PALETTE.mineFriendly : PALETTE.mine;
      this.entityGraphics
        .circle(sp.x, sp.y, 4)
        .fill({ color, alpha: 0.75 })
        .stroke({ width: 1, color, alpha: 0.6 });
    }
  }

  private drawProjectiles(frame: GodViewFrame, t: CameraTransform): void {
    for (const proj of frame.projectiles) {
      const sp = worldToScreen(proj.pos, t);
      this.entityGraphics
        .circle(sp.x, sp.y, 3)
        .fill({ color: PALETTE.projectile, alpha: 0.95 });
    }
  }

  private drawSingularities(frame: GodViewFrame, t: CameraTransform): void {
    for (const sing of frame.singularities) {
      const sp = worldToScreen(sing.pos, t);
      const r = sing.radius * t.scale;
      this.entityGraphics
        .circle(sp.x, sp.y, r)
        .stroke({ width: 2, color: PALETTE.singularity, alpha: 0.7 });
      this.entityGraphics
        .circle(sp.x, sp.y, 5)
        .fill({ color: PALETTE.singularity });
    }
  }

  private drawShips(frame: GodViewFrame, t: CameraTransform): void {
    for (const ship of frame.ships) {
      const sp = worldToScreen(ship.pos, t);
      const color = ship.invuln
        ? PALETTE.shipInvuln
        : ship.alive
          ? PALETTE.ship
          : PALETTE.shipDead;

      // Ship body: equilateral triangle pointing in heading direction
      const size = 7;
      const heading = ship.heading;
      const tip = {
        x: sp.x + Math.cos(heading) * size * 1.6,
        y: sp.y + Math.sin(heading) * size * 1.6,
      };
      const left = {
        x: sp.x + Math.cos(heading + (2.3 * Math.PI) / 3) * size,
        y: sp.y + Math.sin(heading + (2.3 * Math.PI) / 3) * size,
      };
      const right = {
        x: sp.x + Math.cos(heading - (2.3 * Math.PI) / 3) * size,
        y: sp.y + Math.sin(heading - (2.3 * Math.PI) / 3) * size,
      };

      this.entityGraphics
        .poly([tip.x, tip.y, left.x, left.y, right.x, right.y])
        .fill({ color, alpha: ship.alive ? 0.9 : 0.4 })
        .stroke({ width: 1, color: PALETTE.shipHeading, alpha: 0.6 });

      // Relic carry indicator: small dot above ship
      if (ship.relicsCarried > 0) {
        this.entityGraphics
          .circle(sp.x, sp.y - 12, 3)
          .fill({ color: PALETTE.relic });
      }
    }
  }

  destroy(): void {
    this.app.destroy(true);
  }
}
