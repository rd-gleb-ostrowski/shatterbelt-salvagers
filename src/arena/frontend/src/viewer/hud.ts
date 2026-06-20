/**
 * HudOverlay — DOM-based HUD overlay for the Shatterbelt Salvagers viewer.
 *
 * Renders three panels as absolutely-positioned HTML over the PixiJS canvas:
 *   • Match timer        — top-centre, live countdown
 *   • Live scoreboard    — top-left, per-team score + relics carried
 *   • TrueSkill ladder   — top-right, shown between matches
 *
 * Formatting logic lives in the pure functions in hudFormatters.ts.
 * This module owns only DOM creation, event wiring, and fetch().
 *
 * Seam for issue 07 (replay): call hud.update(frame) with a recorded frame;
 * the timer + scoreboard update identically since they are pure-function
 * driven.  The ladder stays independent (it's always a live server fetch).
 */

import { formatTimer, scoreboardRows, ladderRows } from "../lib/hudFormatters.ts";
import type { LadderStanding } from "../lib/hudFormatters.ts";
import type { GodViewFrame } from "../lib/frameParser.ts";

// ── HUD overlay class ─────────────────────────────────────────────────────────

export class HudOverlay {
  private readonly timerEl: HTMLElement;
  private readonly scoreboardEl: HTMLElement;
  private readonly ladderBodyEl: HTMLElement;
  private readonly ladderPanelEl: HTMLElement;

  /**
   * Creates and attaches the HUD DOM tree to `container`.
   *
   * @param container  The element to append the HUD root to (typically
   *                   `document.body` or the viewer wrapper).
   */
  constructor(container: HTMLElement) {
    // ── Root ─────────────────────────────────────────────────────────────────
    const root = document.createElement("div");
    root.id = "hud-overlay";

    // ── Timer ─────────────────────────────────────────────────────────────────
    this.timerEl = document.createElement("div");
    this.timerEl.id = "hud-timer";
    this.timerEl.textContent = "–:––";

    // ── Scoreboard ────────────────────────────────────────────────────────────
    const scoreboardPanel = document.createElement("div");
    scoreboardPanel.id = "hud-scoreboard";
    const scoreboardTitle = document.createElement("div");
    scoreboardTitle.className = "hud-panel-title";
    scoreboardTitle.textContent = "⬡ SCORE";
    this.scoreboardEl = document.createElement("div");
    this.scoreboardEl.id = "hud-scoreboard-rows";
    scoreboardPanel.append(scoreboardTitle, this.scoreboardEl);

    // ── Ladder panel ──────────────────────────────────────────────────────────
    this.ladderPanelEl = document.createElement("div");
    this.ladderPanelEl.id = "hud-ladder";
    const ladderTitle = document.createElement("div");
    ladderTitle.className = "hud-panel-title";
    ladderTitle.textContent = "⬡ LADDER";
    const ladderHeader = document.createElement("div");
    ladderHeader.className = "hud-ladder-header";
    ladderHeader.innerHTML =
      '<span class="hud-rank">#</span>' +
      '<span class="hud-competitor">Competitor</span>' +
      '<span class="hud-skill">Skill</span>' +
      '<span class="hud-matches">M</span>';
    this.ladderBodyEl = document.createElement("div");
    this.ladderBodyEl.id = "hud-ladder-rows";
    this.ladderPanelEl.append(ladderTitle, ladderHeader, this.ladderBodyEl);

    root.append(this.timerEl, scoreboardPanel, this.ladderPanelEl);
    container.appendChild(root);

    // Show ladder initially (no live match yet)
    this.setLadderVisible(true);
  }

  // ── Public API ──────────────────────────────────────────────────────────────

  /**
   * Update the timer and scoreboard from the latest god-view frame.
   *
   * Call this on every received frame from the observer WebSocket (or from
   * the replay player for issue 07 seam).
   */
  update(frame: GodViewFrame): void {
    // Timer
    this.timerEl.textContent = formatTimer(frame.tick, frame.maxTicks);

    // Scoreboard
    const rows = scoreboardRows(frame);
    this.scoreboardEl.innerHTML = "";
    for (const row of rows) {
      const colourCss = `#${row.colour.toString(16).padStart(6, "0")}`;
      const el = document.createElement("div");
      el.className = "hud-score-row";
      el.innerHTML =
        `<span class="hud-team-dot" style="color:${colourCss}">●</span>` +
        `<span class="hud-team-name">${escapeHtml(row.team)}</span>` +
        `<span class="hud-score">${row.score}</span>` +
        `<span class="hud-relics">${row.relicsCarried > 0 ? `+${row.relicsCarried}⬡` : ""}</span>`;
      this.scoreboardEl.appendChild(el);
    }

    // Ladder: visible when match is over (tick ≥ maxTicks); hidden during play
    this.setLadderVisible(frame.tick >= frame.maxTicks);
  }

  /**
   * Fetch standings from GET /ladder/standings and populate the ladder panel.
   *
   * Silently ignores network errors so the HUD degrades gracefully when the
   * ladder endpoint is unavailable.  Call this once on startup and optionally
   * after each match ends.
   */
  async fetchLadder(): Promise<void> {
    let data: unknown;
    try {
      const res = await fetch("/ladder/standings");
      if (!res.ok) return;
      data = await res.json();
    } catch {
      return; // network unavailable — ladder stays empty
    }

    if (!Array.isArray(data)) return;
    const rows = ladderRows(data as LadderStanding[]);

    this.ladderBodyEl.innerHTML = "";
    for (let i = 0; i < rows.length; i++) {
      const row = rows[i]!;
      const el = document.createElement("div");
      el.className = "hud-ladder-row";
      el.innerHTML =
        `<span class="hud-rank">${i + 1}</span>` +
        `<span class="hud-competitor">${escapeHtml(row.competitor)}</span>` +
        `<span class="hud-skill">${row.conservativeSkill}</span>` +
        `<span class="hud-matches">${row.matches}</span>`;
      this.ladderBodyEl.appendChild(el);
    }
  }

  // ── Private helpers ──────────────────────────────────────────────────────────

  private setLadderVisible(visible: boolean): void {
    this.ladderPanelEl.style.display = visible ? "block" : "none";
  }
}

// ── Utility ───────────────────────────────────────────────────────────────────

/** Minimal HTML escaping for competitor/team names coming from the server. */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
