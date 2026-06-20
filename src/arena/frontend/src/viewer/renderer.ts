/**
 * renderer — PixiJS WebGL renderer for the Drift Viewer.
 *
 * Renders: Drift bounds, asteroids, ships (with heading indicator),
 * relics (glowing), Anchors (team home beacons), rune-cannon projectiles,
 * singularity gravity wells, deployed Aether Mines, and transient explosions.
 *
 * Design:
 * - All entity graphics are drawn to a single Graphics object per tick
 *   (cleared + redrawn) for simplicity in this first slice.
 * - The world->screen transform is produced by the Camera model (camera.ts),
 *   which replaced the direct fitDriftTransform call from issue 01.
 * - Layer order (bottom → top):
 *     driftGraphics  — arena border
 *     glowGraphics   — additive glow halos (relics, anchors, singularities,
 *                      explosion blooms)
 *     entityGraphics — solid game entities
 *     effectsGraphics — transient effects (explosions) rendered above entities
 *     labelContainer — ship name labels always on top
 *
 * KNOWN LIMITATION — Arc Lance beams (issue 04 seam):
 *   Arc Lance is emitted by the engine as an event (a one-shot discharge),
 *   not as a persistent entity.  The god-view frame contains only STATE arrays
 *   (singularities, mines, projectiles); it does NOT include an `events` field.
 *   Therefore Arc Lance beams CANNOT be drawn from god-view state in v1.
 *   The `effectsGraphics` layer is the correct place to render them if/when
 *   the server adds Arc Lance to the god-view event stream.
 *   See also: PROTOCOL.md / observer.rs — the same gap noted in issues 01 + 06.
 *
 * Seams for future issues:
 *   05  — add HUD overlay (scoreboard, tick timer); read camera.followTarget
 *   06  — sound module reuses `detectExplosions` return value from renderFrame
 *         to trigger explosion/sigil SFX without duplicating delta detection
 *   07  — replay loader feeds the same `renderFrame` callback from recorded
 *         frames; the effectsModel and explosionDetector advance naturally
 */

import { Application, Graphics, Container, Text } from "pixi.js";
import type { GodViewFrame, GodShipView } from "../lib/frameParser.ts";
import {
  worldToScreen,
  type CameraTransform,
} from "../lib/worldTransform.ts";
import { Camera } from "../lib/camera.ts";
import { teamColour, barFillRatio, isThrusting } from "../lib/shipPresentation.ts";
import { detectExplosions } from "../lib/explosionDetector.ts";
import {
  EMPTY_EFFECTS,
  addExplosions,
  advanceEffects,
  type EffectsState,
} from "../lib/effectsModel.ts";

// ── Palette ───────────────────────────────────────────────────────────────────

const PALETTE = {
  driftBg: 0x05040f,
  driftBorder: 0x2a2050,
  asteroid: 0x5a5070,
  asteroidStroke: 0x8070a0,
  shipDead: 0x334455,
  shipHeading: 0xffffff,
  shipInvulnRing: 0xd0a0ff,
  shipInvulnFill: 0xd0a0ff,
  thrustNormal: 0xff6600,
  thrustAfterburner: 0xffaa00,
  hullBarBg: 0x220000,
  hullBarFill: 0x44ff44,
  shieldBarBg: 0x000022,
  shieldBarFill: 0x4488ff,
  anchor: 0xffcc44,
  anchorGlow: 0xffaa00,
  relic: 0x44ffaa,
  relicGlow: 0x00ff88,
  projectile: 0xffd060,
  singularity: 0xcc44ff,
  mine: 0xff4444,
  mineFriendly: 0x44aaff,
  explosionCore: 0xffffff,
  explosionMid: 0xff8800,
  explosionOuter: 0xff2200,
} as const;

// ── DriftRenderer ─────────────────────────────────────────────────────────────

export class DriftRenderer {
  private app: Application;
  private driftGraphics: Graphics;
  private entityGraphics: Graphics;
  private glowGraphics: Graphics;
  /**
   * Transient effects layer — rendered above entityGraphics, below labels.
   * Used for explosions; the correct place for Arc Lance beams when the
   * server exposes them in the god-view stream.
   */
  private effectsGraphics: Graphics;
  /** Container for per-ship name labels; cleared and rebuilt every frame. */
  private labelContainer: Container;

  /** Camera instance — main.ts wires input handlers against this object. */
  readonly camera: Camera = new Camera();

  /** Ships from the most-recently-rendered frame, used for explosion delta. */
  private prevShips: readonly GodShipView[] = [];
  /** Active transient effects (explosions). */
  private effectsState: EffectsState = EMPTY_EFFECTS;

  private constructor(
    app: Application,
    driftGraphics: Graphics,
    entityGraphics: Graphics,
    glowGraphics: Graphics,
    effectsGraphics: Graphics,
    labelContainer: Container,
  ) {
    this.app = app;
    this.driftGraphics = driftGraphics;
    this.entityGraphics = entityGraphics;
    this.glowGraphics = glowGraphics;
    this.effectsGraphics = effectsGraphics;
    this.labelContainer = labelContainer;
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

    // Layer order: drift bg → glows (additive) → entities → effects → labels on top
    const worldLayer = new Container();
    const driftGraphics = new Graphics();
    const glowGraphics = new Graphics();
    const entityGraphics = new Graphics();
    const effectsGraphics = new Graphics();
    const labelContainer = new Container();

    worldLayer.addChild(driftGraphics);
    worldLayer.addChild(glowGraphics);
    worldLayer.addChild(entityGraphics);
    worldLayer.addChild(effectsGraphics);
    worldLayer.addChild(labelContainer);
    app.stage.addChild(worldLayer);

    return new DriftRenderer(
      app,
      driftGraphics,
      entityGraphics,
      glowGraphics,
      effectsGraphics,
      labelContainer,
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
   *
   * Also: detects ship destructions via frame delta, advances the effects
   * model, and returns the new explosion events (seam for issue 06 sound).
   */
  renderFrame(frame: GodViewFrame): void {
    const canvas = { width: this.canvasWidth, height: this.canvasHeight };

    // Resolve ship position for follow mode
    const followTarget = this.camera.followTarget;
    const shipPos = followTarget !== null
      ? frame.ships.find(s => s.id === followTarget)?.pos
      : undefined;

    const transform = this.camera.getTransform(frame.arena, canvas, shipPos);

    // ── Explosion detection (pure delta) ─────────────────────────────────────
    const newExplosions = detectExplosions(this.prevShips, frame.ships);
    this.effectsState = advanceEffects(
      addExplosions(this.effectsState, newExplosions),
    );
    this.prevShips = frame.ships;

    this.drawDrift(frame, transform);
    this.drawGlows(frame, transform);
    this.drawEntities(frame, transform);
    this.drawEffects(transform);
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

    // Explosion bloom halos — drawn in the glow layer for additive feel
    for (const fx of this.effectsState.effects) {
      if (fx.kind !== "explosion") continue;
      const sp = worldToScreen(fx.pos, t);
      // Fade out as the effect ages: alpha proportional to remaining life
      const life = fx.ticksLeft / 20; // normalised 0→1
      this.glowGraphics
        .circle(sp.x, sp.y, 40 * life + 4)
        .fill({ color: PALETTE.explosionOuter, alpha: 0.18 * life });
      this.glowGraphics
        .circle(sp.x, sp.y, 22 * life + 2)
        .fill({ color: PALETTE.explosionMid, alpha: 0.28 * life });
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
    // Clear name labels from the previous frame
    const removed = this.labelContainer.removeChildren();
    for (const child of removed) child.destroy();

    for (const ship of frame.ships) {
      const sp = worldToScreen(ship.pos, t);
      const colour = ship.alive ? teamColour(ship.id) : PALETTE.shipDead;
      const alpha = ship.alive ? 0.9 : 0.4;
      const size = 7;
      const heading = ship.heading;

      // Triangle vertices — tip points in heading direction
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

      // ── Thrust flame — drawn behind the ship (opposite to heading) ────────
      // Rule: afterburnerTicksLeft > 0 (afterburner Sigil) → bright orange
      //       forward speed > threshold               → normal orange flame
      if (ship.alive && isThrusting(ship)) {
        const flameDir = heading + Math.PI;
        const flameLen = ship.afterburnerTicksLeft > 0 ? 18 : 12;
        const flameW = ship.afterburnerTicksLeft > 0 ? 5 : 3.5;
        const flameColour = ship.afterburnerTicksLeft > 0
          ? PALETTE.thrustAfterburner
          : PALETTE.thrustNormal;
        const flameAlpha = ship.afterburnerTicksLeft > 0 ? 0.95 : 0.7;

        const flameTip = {
          x: sp.x + Math.cos(flameDir) * flameLen,
          y: sp.y + Math.sin(flameDir) * flameLen,
        };
        // Flame base — two points straddling the rear of the ship body
        const flBaseAngle = heading + (2.5 * Math.PI) / 3;
        const flameLeft = {
          x: sp.x + Math.cos(flBaseAngle) * flameW,
          y: sp.y + Math.sin(flBaseAngle) * flameW,
        };
        const flameRight = {
          x: sp.x + Math.cos(heading - (2.5 * Math.PI) / 3) * flameW,
          y: sp.y + Math.sin(heading - (2.5 * Math.PI) / 3) * flameW,
        };
        this.entityGraphics
          .poly([flameTip.x, flameTip.y, flameLeft.x, flameLeft.y, flameRight.x, flameRight.y])
          .fill({ color: flameColour, alpha: flameAlpha });
      }

      // ── Invuln shimmer — pulsing ring when spawn protection is active ──────
      // Disappears automatically when the invuln flag clears (issue 03 AC).
      if (ship.alive && ship.invuln) {
        const shimmerAlpha = 0.3 + Math.sin(frame.tick * 0.3) * 0.25;
        this.entityGraphics
          .circle(sp.x, sp.y, size * 2.4)
          .stroke({ width: 2.5, color: PALETTE.shipInvulnRing, alpha: Math.max(0.05, shimmerAlpha) });
        this.entityGraphics
          .circle(sp.x, sp.y, size * 1.6)
          .fill({ color: PALETTE.shipInvulnFill, alpha: 0.1 });
      }

      // ── Ship body — team-coloured triangle oriented by heading ─────────────
      this.entityGraphics
        .poly([tip.x, tip.y, left.x, left.y, right.x, right.y])
        .fill({ color: colour, alpha })
        .stroke({ width: 1, color: PALETTE.shipHeading, alpha: 0.6 });

      if (ship.alive) {
        // ── Hull bar (green) ────────────────────────────────────────────────
        const barW = 20;
        const barH = 3;
        const barX = sp.x - barW / 2;
        const hullBarY = sp.y + size * 2;

        this.entityGraphics
          .rect(barX, hullBarY, barW, barH)
          .fill({ color: PALETTE.hullBarBg, alpha: 0.7 });
        const hf = barFillRatio(ship.hull.cur, ship.hull.max);
        if (hf > 0) {
          this.entityGraphics
            .rect(barX, hullBarY, barW * hf, barH)
            .fill({ color: PALETTE.hullBarFill, alpha: 0.85 });
        }

        // ── Shield bar (blue) — one pixel below hull bar ────────────────────
        const shieldBarY = hullBarY + barH + 1;
        this.entityGraphics
          .rect(barX, shieldBarY, barW, barH)
          .fill({ color: PALETTE.shieldBarBg, alpha: 0.7 });
        const sf = barFillRatio(ship.shield.cur, ship.shield.max);
        if (sf > 0) {
          this.entityGraphics
            .rect(barX, shieldBarY, barW * sf, barH)
            .fill({ color: PALETTE.shieldBarFill, alpha: 0.85 });
        }

        // ── Name label — team colour, centered above ship ───────────────────
        const colourStr = `#${colour.toString(16).padStart(6, "0")}`;
        const label = new Text({
          text: ship.id,
          style: {
            fontFamily: "monospace",
            fontSize: 10,
            fill: colourStr,
          },
        });
        label.anchor.set(0.5, 1);
        label.x = sp.x;
        label.y = sp.y - size * 2.5;
        this.labelContainer.addChild(label);
      }

      // ── Relic carry indicator — small dot above ship ───────────────────────
      if (ship.relicsCarried > 0) {
        this.entityGraphics
          .circle(sp.x, sp.y - 12, 3)
          .fill({ color: PALETTE.relic });
      }
    }
  }

  // ── Layer: Transient effects (explosions) ─────────────────────────────────

  /**
   * Draw all active transient effects onto the effectsGraphics layer.
   *
   * Currently renders explosions as a bright core + decaying ring.
   *
   * Arc Lance beams would be drawn here when the server adds them to the
   * god-view stream — see the KNOWN LIMITATION comment at the top of this file.
   */
  private drawEffects(t: CameraTransform): void {
    this.effectsGraphics.clear();

    for (const fx of this.effectsState.effects) {
      if (fx.kind !== "explosion") continue;
      const sp = worldToScreen(fx.pos, t);
      // Normalised life fraction (1 = fresh, approaching 0 = fading)
      const life = fx.ticksLeft / 20;

      // Outer ring — expands and fades
      const outerR = 6 + (1 - life) * 18;
      this.effectsGraphics
        .circle(sp.x, sp.y, outerR)
        .stroke({ width: 2, color: PALETTE.explosionOuter, alpha: 0.7 * life });

      // Mid burst — shrinks as it fades
      this.effectsGraphics
        .circle(sp.x, sp.y, 10 * life + 2)
        .fill({ color: PALETTE.explosionMid, alpha: 0.6 * life });

      // Bright core — only visible in first half of lifetime
      if (life > 0.5) {
        this.effectsGraphics
          .circle(sp.x, sp.y, 5 * life)
          .fill({ color: PALETTE.explosionCore, alpha: (life - 0.5) * 2 });
      }
    }
  }

  destroy(): void {
    this.app.destroy(true);
  }
}
