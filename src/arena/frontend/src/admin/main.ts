/**
 * Admin entry point — issue 08: auth shell + bot health dashboard.
 *
 * Architecture:
 *   - Sign-in form: password entered once, verified via GET /admin/bots,
 *     stored in-memory by session.ts; never sent to the server until verified.
 *   - Bot health dashboard: table from GET /admin/bots, polled every 5 s.
 *   - Pure logic (API client, formatters) lives in src/admin/lib/ and is
 *     unit-tested. The DOM layer here is manual/visual per the PRD.
 *
 * Seam for issues 09-12: import { getSession } from "./session.ts" and call
 * session.client.<method>() — the auth header is already baked in.
 */

import { createAdminClient, type BotHealthSnapshot } from "./lib/adminClient.ts";
import type { StartMatchResult, LadderStanding, RecordingListItem } from "./lib/adminClient.ts";
import { setSession, getSession, clearSession, getStoredPassword } from "./session.ts";
import {
  formatLastSeen,
  formatConnected,
  formatKind,
} from "./lib/adminFormatters.ts";

// ── Constants ─────────────────────────────────────────────────────────────────

/** Same-origin base URL. Override to "http://localhost:3000" during dev if needed. */
const BASE_URL = "";
const POLL_INTERVAL_MS = 5_000;

// ── State ─────────────────────────────────────────────────────────────────────

let pollTimer: ReturnType<typeof setInterval> | null = null;

// ── Utility ───────────────────────────────────────────────────────────────────

function escHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ── Sign-in view ──────────────────────────────────────────────────────────────

function renderSignIn(errorMsg?: string): void {
  stopPolling();

  const app = getApp();
  app.innerHTML = `
    <section class="signin-card">
      <h2>Facilitator sign in</h2>
      <form id="signin-form" autocomplete="off">
        <label for="pw-input">Password</label>
        <input
          type="password"
          id="pw-input"
          name="password"
          autocomplete="current-password"
          placeholder="Facilitator password"
          required
        />
        <button type="submit" id="signin-btn">Sign in</button>
      </form>
      ${errorMsg ? `<p class="error" id="signin-error">${escHtml(errorMsg)}</p>` : `<p id="signin-error" hidden></p>`}
    </section>
  `;

  const form = document.getElementById("signin-form") as HTMLFormElement;
  const input = document.getElementById("pw-input") as HTMLInputElement;
  const btn = document.getElementById("signin-btn") as HTMLButtonElement;

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const pw = input.value;
    if (!pw) return;

    btn.disabled = true;
    btn.textContent = "Verifying…";

    try {
      const client = createAdminClient(BASE_URL, pw);
      const result = await client.verifyAuth();

      if (!result.ok) {
        btn.disabled = false;
        btn.textContent = "Sign in";
        const msg = result.unauthorized
          ? "Wrong password — access denied."
          : "Could not reach the server. Is it running?";
        renderSignIn(msg);
        return;
      }

      setSession(pw, client);
      renderDashboard();
    } catch {
      btn.disabled = false;
      btn.textContent = "Sign in";
      renderSignIn("Network error — could not connect to the server.");
    }
  });

  input.focus();
}

// ── Dashboard view ────────────────────────────────────────────────────────────

function renderDashboard(): void {
  const app = getApp();
  app.innerHTML = `
    <nav class="admin-nav">
      <span class="nav-title">⬡ Arena Admin</span>
      <button id="signout-btn" class="btn-secondary">Sign out</button>
    </nav>

    <section class="panel" id="health-panel">
      <h2>Bot Health</h2>
      <p id="health-status" class="status-line">Loading…</p>
      <div class="table-wrap">
        <table id="bots-table" hidden>
          <thead>
            <tr>
              <th>Team</th>
              <th>Kind</th>
              <th>Status</th>
              <th>Last seen</th>
              <th>Skipped ticks</th>
              <th>Crashes</th>
              <th>Recent logs</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody id="bots-body"></tbody>
        </table>
      </div>
    </section>

    <section class="panel" id="default-bot-panel">
      <h2>Default Bot</h2>
      <p class="hint">
        The Default Bot fills empty slots. Upload a custom WASM artifact to
        override the built-in heuristic, or clear it to revert.
      </p>
      <div class="default-bot-controls">
        <label class="file-label">
          Choose .wasm
          <input type="file" id="default-bot-file" accept=".wasm" />
        </label>
        <button id="set-default-bot-btn" class="btn-primary">Set Default Bot</button>
        <button id="clear-default-bot-btn" class="btn-secondary">Clear Default Bot</button>
      </div>
      <p id="default-bot-status" class="status-line" hidden></p>
    </section>

    <!-- Seam: issues 09-12 inject their panels here -->
    <div id="admin-panels"></div>
  `;

  document.getElementById("signout-btn")!.addEventListener("click", () => {
    stopPolling();
    clearSession();
    renderSignIn();
  });

  wireDefaultBotControls();
  startPolling();
  renderMatchControlPanel();
  renderLadderPanel();
  renderReplaysPanel();
}

function renderBotsTable(bots: BotHealthSnapshot[]): void {
  const table = document.getElementById("bots-table");
  const statusEl = document.getElementById("health-status");
  const tbody = document.getElementById("bots-body");
  if (!table || !statusEl || !tbody) return;

  tbody.innerHTML = "";

  if (bots.length === 0) {
    statusEl.textContent = "No bots registered yet.";
    table.hidden = true;
    return;
  }

  const time = new Date().toLocaleTimeString();
  statusEl.textContent = `${bots.length} bot(s) — updated ${time}`;

  for (const bot of bots) {
    const tr = document.createElement("tr");
    if (!bot.connected) tr.classList.add("row-offline");

    const logSnippet = bot.recentLogs.slice(-500);
    tr.innerHTML = `
      <td class="col-team">${escHtml(bot.team)}</td>
      <td class="col-kind">${escHtml(formatKind(bot.kind))}</td>
      <td class="col-status ${bot.connected ? "connected" : "offline"}">${escHtml(formatConnected(bot.connected))}</td>
      <td class="col-lastseen">${escHtml(formatLastSeen(bot.lastSeen))}</td>
      <td class="col-skipped">${bot.skippedTicks}</td>
      <td class="col-crashes">${bot.crashes}</td>
      <td class="col-logs"><pre class="log-pre">${escHtml(logSnippet) || "<em class='no-logs'>—</em>"}</pre></td>
      <td class="col-actions">
        <button class="btn-kick" data-team="${escHtml(bot.team)}">Kick</button>
        <button class="btn-disable" data-team="${escHtml(bot.team)}">Disable</button>
        <button class="btn-enable" data-team="${escHtml(bot.team)}">Enable</button>
        <label class="file-label-inline">
          Upload WASM
          <input type="file" accept=".wasm" class="file-input-bot" data-team="${escHtml(bot.team)}" />
        </label>
      </td>
    `;
    tbody.appendChild(tr);

    // Wire the per-row file input directly (not delegated — easier with file inputs)
    const fileInput = tr.querySelector(".file-input-bot") as HTMLInputElement;
    fileInput.addEventListener("change", () => handleBotUploadChange(fileInput));
  }

  table.hidden = false;

  // Delegate click events for Kick buttons
  tbody.addEventListener("click", handleKickClick);
  // Delegate click events for Disable/Enable buttons
  tbody.addEventListener("click", handleBotToggleClick);
}

async function handleKickClick(e: Event): Promise<void> {
  const btn = (e.target as HTMLElement).closest(".btn-kick") as HTMLButtonElement | null;
  if (!btn) return;

  const team = btn.dataset.team;
  if (!team) return;

  const confirmed = window.confirm(
    `Kick "${team}"?\n\nThis will disqualify the bot and remove it from the current match.`,
  );
  if (!confirmed) return;

  const session = getSession();
  if (!session) return;

  btn.disabled = true;
  btn.textContent = "Kicking…";

  try {
    const result = await session.client.kickBot(team);

    if (result.unauthorized) {
      btn.disabled = false;
      btn.textContent = "Kick";
      alert("Action denied — session may have expired. Please sign out and sign in again.");
      return;
    }

    if (!result.ok) {
      btn.disabled = false;
      btn.textContent = "Kick";
      alert(`Failed to kick "${team}" — server returned an error.`);
      return;
    }

    // Optimistically mark the row as offline while the next poll confirms it
    const row = btn.closest("tr");
    if (row) {
      row.classList.add("row-offline");
      const statusCell = row.querySelector(".col-status");
      if (statusCell) {
        statusCell.textContent = "Disqualified";
        statusCell.className = "col-status offline";
      }
    }
    btn.textContent = "Kicked";
  } catch {
    btn.disabled = false;
    btn.textContent = "Kick";
    alert(`Network error while kicking "${team}".`);
  }
}

async function handleBotToggleClick(e: Event): Promise<void> {
  const btn = (e.target as HTMLElement).closest(
    ".btn-disable, .btn-enable",
  ) as HTMLButtonElement | null;
  if (!btn) return;

  const team = btn.dataset.team;
  if (!team) return;

  const isDisable = btn.classList.contains("btn-disable");
  const action = isDisable ? "disable" : "enable";

  const session = getSession();
  if (!session) return;

  btn.disabled = true;
  btn.textContent = isDisable ? "Disabling…" : "Enabling…";

  try {
    const result = isDisable
      ? await session.client.disableBot(team)
      : await session.client.enableBot(team);

    if (result.unauthorized) {
      btn.disabled = false;
      btn.textContent = isDisable ? "Disable" : "Enable";
      alert("Action denied — session may have expired. Please sign out and sign in again.");
      return;
    }

    if (!result.ok) {
      btn.disabled = false;
      btn.textContent = isDisable ? "Disable" : "Enable";
      alert(`Failed to ${action} "${team}" — server returned an error.`);
      return;
    }

    btn.disabled = false;
    btn.textContent = isDisable ? "Disable" : "Enable";
    // Next poll will reflect the updated state in the health snapshot
  } catch {
    btn.disabled = false;
    btn.textContent = isDisable ? "Disable" : "Enable";
    alert(`Network error while trying to ${action} "${team}".`);
  }
}

async function handleBotUploadChange(fileInput: HTMLInputElement): Promise<void> {
  const team = fileInput.dataset.team;
  if (!team || !fileInput.files?.length) return;

  const file = fileInput.files[0];
  const session = getSession();
  if (!session) return;

  fileInput.disabled = true;

  try {
    const wasm = await file.arrayBuffer();
    const result = await session.client.uploadTeamBot(team, wasm);

    if (result.unauthorized) {
      alert("Upload denied — session may have expired. Please sign out and sign in again.");
    } else if (result.badRequest) {
      alert(`Upload failed for "${team}" — file is not a valid WASM artifact.`);
    } else if (!result.ok) {
      alert(`Upload failed for "${team}" — server returned an error.`);
    } else {
      alert(`Bot uploaded successfully for "${team}".`);
    }
  } catch {
    alert(`Network error while uploading bot for "${team}".`);
  } finally {
    fileInput.disabled = false;
    fileInput.value = "";
  }
}

// ── Default Bot controls ──────────────────────────────────────────────────────

function wireDefaultBotControls(): void {
  const fileInput = document.getElementById("default-bot-file") as HTMLInputElement | null;
  const setBtn = document.getElementById("set-default-bot-btn") as HTMLButtonElement | null;
  const clearBtn = document.getElementById("clear-default-bot-btn") as HTMLButtonElement | null;
  const statusEl = document.getElementById("default-bot-status") as HTMLElement | null;
  if (!fileInput || !setBtn || !clearBtn || !statusEl) return;

  function showStatus(msg: string): void {
    if (!statusEl) return;
    statusEl.textContent = msg;
    statusEl.hidden = false;
  }

  setBtn.addEventListener("click", async () => {
    if (!fileInput.files?.length) {
      alert("Please choose a .wasm file first.");
      return;
    }
    const file = fileInput.files[0];
    const session = getSession();
    if (!session) return;

    setBtn.disabled = true;
    setBtn.textContent = "Uploading…";

    try {
      const wasm = await file.arrayBuffer();
      const result = await session.client.setDefaultBot(wasm);

      if (result.unauthorized) {
        showStatus("⚠ Denied — session may have expired.");
      } else if (result.badRequest) {
        showStatus("⚠ Upload rejected — not a valid WASM artifact.");
      } else if (!result.ok) {
        showStatus("⚠ Upload failed — server returned an error.");
      } else {
        showStatus("✓ Default Bot updated.");
        fileInput.value = "";
      }
    } catch {
      showStatus("⚠ Network error during upload.");
    } finally {
      setBtn.disabled = false;
      setBtn.textContent = "Set Default Bot";
    }
  });

  clearBtn.addEventListener("click", async () => {
    const session = getSession();
    if (!session) return;

    clearBtn.disabled = true;
    clearBtn.textContent = "Clearing…";

    try {
      const result = await session.client.clearDefaultBot();

      if (result.unauthorized) {
        showStatus("⚠ Denied — session may have expired.");
      } else if (!result.ok) {
        showStatus("⚠ Clear failed — server returned an error.");
      } else {
        showStatus("✓ Default Bot cleared (reverted to built-in).");
      }
    } catch {
      showStatus("⚠ Network error during clear.");
    } finally {
      clearBtn.disabled = false;
      clearBtn.textContent = "Clear Default Bot";
    }
  });
}

// ── Polling ───────────────────────────────────────────────────────────────────

function startPolling(): void {
  doPoll();
  pollTimer = setInterval(doPoll, POLL_INTERVAL_MS);
}

function stopPolling(): void {
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

async function doPoll(): Promise<void> {
  const session = getSession();
  if (!session) return;

  try {
    const bots = await session.client.getBots();
    renderBotsTable(bots);
  } catch {
    const statusEl = document.getElementById("health-status");
    if (statusEl) {
      statusEl.textContent = "⚠ Error fetching bot health — retrying in 5 s…";
    }
  }
}

// ── Match Control panel ───────────────────────────────────────────────────────

/**
 * Inject the match-control panel into #admin-panels.
 *
 * Panel sections:
 *   1. Start Match form (mode, seed, maxTicks, teams, tps).
 *   2. Live Match Controls (pause/resume/tps/abort) — shown after a live match
 *      is started, or by entering a known match ID.
 *   3. Exhibition controls (start/stop/status) — the projector is never blank.
 *   4. Open Viewer link — opens /index.html which connects to /observe.
 *
 * Push-to-projector mapping:
 *   The server streams the current live match to `/observe` automatically.
 *   Starting a live match IS the push-to-projector action. The Viewer (at
 *   /index.html, connecting to /observe) will pick it up immediately.
 *   There is no separate push endpoint; exhibition keeps /observe live when
 *   no on-demand match is running.
 */
function renderMatchControlPanel(): void {
  const container = document.getElementById("admin-panels");
  if (!container) return;

  const panel = document.createElement("section");
  panel.className = "panel";
  panel.id = "match-control-panel";
  panel.innerHTML = `
    <h2>Match Control</h2>

    <!-- 1. Start Match -->
    <div class="subpanel" id="start-match-subpanel">
      <h3>Start Match</h3>
      <p class="hint">
        Starting a <strong>live</strong> match streams it to the projector Viewer
        at <code>/observe</code> automatically — no separate push needed.
      </p>
      <form id="start-match-form" autocomplete="off">
        <label for="match-mode">Mode</label>
        <select id="match-mode" name="mode">
          <option value="live" selected>Live (streams to Viewer)</option>
          <option value="headless">Headless (instant, no stream)</option>
        </select>

        <label for="match-seed">Seed <span class="hint">(optional)</span></label>
        <input type="number" id="match-seed" name="seed" placeholder="leave blank for server default" />

        <label for="match-max-ticks">Max Ticks <span class="hint">(optional)</span></label>
        <input type="number" id="match-max-ticks" name="maxTicks" placeholder="leave blank for server default" min="1" />

        <label for="match-tps">TPS <span class="hint">(live only, optional, default 30)</span></label>
        <input type="number" id="match-tps" name="tps" placeholder="30" min="1" max="400" />

        <label for="match-teams">Teams <span class="hint">(optional, comma-separated)</span></label>
        <input type="text" id="match-teams" name="teams" placeholder="alpha, beta, gamma …" />

        <button type="submit" id="start-match-btn" class="btn-primary">Start Match</button>
      </form>
      <p id="start-match-status" class="status-line" hidden></p>
    </div>

    <!-- 2. Live Match Controls -->
    <div class="subpanel" id="live-match-subpanel">
      <h3>Live Match Controls</h3>
      <p class="hint">
        Controls apply to the active match ID below. After starting a live match
        the ID is filled in automatically.
      </p>
      <div class="match-id-row">
        <label for="active-match-id">Match ID</label>
        <input type="text" id="active-match-id" placeholder="UUID of running live match" />
      </div>
      <div class="match-control-buttons">
        <button id="pause-match-btn" class="btn-secondary">⏸ Pause</button>
        <button id="resume-match-btn" class="btn-secondary">▶ Resume</button>
        <button id="abort-match-btn" class="btn-danger">✕ Abort</button>
      </div>
      <div class="tps-row">
        <label for="match-tps-input">Change TPS</label>
        <input type="number" id="match-tps-input" value="30" min="1" max="400" style="width:5em" />
        <button id="set-tps-btn" class="btn-secondary">Set TPS</button>
      </div>
      <p id="live-match-status" class="status-line" hidden></p>
    </div>

    <!-- 3. Exhibition (projector never blank) -->
    <div class="subpanel" id="exhibition-subpanel">
      <h3>Exhibition</h3>
      <p class="hint">
        Exhibition runs an endless series of live matches streaming to
        <code>/observe</code>, keeping the projector Viewer occupied between
        on-demand matches.
      </p>
      <div class="exhibition-controls">
        <button id="start-exhibition-btn" class="btn-primary">▶ Start Exhibition</button>
        <button id="stop-exhibition-btn" class="btn-secondary">⏹ Stop Exhibition</button>
        <button id="refresh-exhibition-btn" class="btn-secondary">↺ Refresh Status</button>
        <label for="exhibition-tps">TPS <span class="hint">(optional, default 30)</span></label>
        <input type="number" id="exhibition-tps" name="tps" placeholder="30" min="1" max="400" />
      </div>
      <p id="exhibition-status" class="status-line">Status: unknown</p>
    </div>

    <!-- 4. Open Viewer (push-to-projector convenience link) -->
    <div class="subpanel" id="viewer-subpanel">
      <h3>Projector Viewer</h3>
      <p class="hint">
        The Viewer connects to <code>/observe</code> and shows whatever live
        match is running. Start a live match or exhibition above, then open the
        Viewer on the projector screen.
      </p>
      <a href="/index.html" target="_blank" rel="noopener" class="btn-primary viewer-link">
        ↗ Open Viewer
      </a>
    </div>
  `;

  container.appendChild(panel);

  wireStartMatchForm();
  wireLiveMatchControls();
  wireExhibitionControls();
}

function wireStartMatchForm(): void {
  const form = document.getElementById("start-match-form") as HTMLFormElement | null;
  const btn = document.getElementById("start-match-btn") as HTMLButtonElement | null;
  const statusEl = document.getElementById("start-match-status") as HTMLElement | null;
  if (!form || !btn || !statusEl) return;

  function showStartStatus(msg: string): void {
    if (!statusEl) return;
    statusEl.textContent = msg;
    statusEl.hidden = false;
  }

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const session = getSession();
    if (!session) return;

    const modeEl = document.getElementById("match-mode") as HTMLSelectElement;
    const seedEl = document.getElementById("match-seed") as HTMLInputElement;
    const maxTicksEl = document.getElementById("match-max-ticks") as HTMLInputElement;
    const tpsEl = document.getElementById("match-tps") as HTMLInputElement;
    const teamsEl = document.getElementById("match-teams") as HTMLInputElement;

    const opts: Parameters<typeof session.client.startMatch>[0] = {
      mode: (modeEl.value as "live" | "headless") || "live",
    };
    if (seedEl.value.trim()) opts.seed = Number(seedEl.value);
    if (maxTicksEl.value.trim()) opts.maxTicks = Number(maxTicksEl.value);
    if (tpsEl.value.trim()) opts.tps = Number(tpsEl.value);
    if (teamsEl.value.trim()) {
      opts.teams = teamsEl.value
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);
    }

    btn.disabled = true;
    btn.textContent = "Starting…";

    try {
      const result: StartMatchResult = await session.client.startMatch(opts);

      if (result.unauthorized) {
        showStartStatus("⚠ Denied — session may have expired. Please sign out and sign in again.");
      } else if (!result.ok) {
        showStartStatus("⚠ Failed to start match — server returned an error.");
      } else {
        showStartStatus(
          `✓ Match started — ID: ${result.matchId} (mode: ${result.mode})`,
        );
        // Auto-populate the live controls ID field for convenience
        if (result.mode === "live") {
          const idInput = document.getElementById("active-match-id") as HTMLInputElement | null;
          if (idInput) idInput.value = result.matchId;
        }
      }
    } catch {
      showStartStatus("⚠ Network error — could not reach the server.");
    } finally {
      btn.disabled = false;
      btn.textContent = "Start Match";
    }
  });
}

function wireLiveMatchControls(): void {
  const pauseBtn = document.getElementById("pause-match-btn") as HTMLButtonElement | null;
  const resumeBtn = document.getElementById("resume-match-btn") as HTMLButtonElement | null;
  const abortBtn = document.getElementById("abort-match-btn") as HTMLButtonElement | null;
  const setTpsBtn = document.getElementById("set-tps-btn") as HTMLButtonElement | null;
  const statusEl = document.getElementById("live-match-status") as HTMLElement | null;
  if (!pauseBtn || !resumeBtn || !abortBtn || !setTpsBtn || !statusEl) return;

  function getMatchId(): string | null {
    const idInput = document.getElementById("active-match-id") as HTMLInputElement | null;
    const id = idInput?.value.trim();
    if (!id) {
      if (statusEl) {
        statusEl.textContent = "⚠ Enter a Match ID above first.";
        statusEl.hidden = false;
      }
      return null;
    }
    return id;
  }

  function showLiveStatus(msg: string): void {
    if (!statusEl) return;
    statusEl.textContent = msg;
    statusEl.hidden = false;
  }

  pauseBtn.addEventListener("click", async () => {
    const session = getSession();
    const id = getMatchId();
    if (!session || !id) return;
    pauseBtn.disabled = true;
    try {
      const r = await session.client.pauseMatch(id);
      showLiveStatus(r.ok ? "✓ Match paused." : r.unauthorized ? "⚠ Denied." : "⚠ Error pausing match.");
    } catch { showLiveStatus("⚠ Network error."); }
    finally { pauseBtn.disabled = false; }
  });

  resumeBtn.addEventListener("click", async () => {
    const session = getSession();
    const id = getMatchId();
    if (!session || !id) return;
    resumeBtn.disabled = true;
    try {
      const r = await session.client.resumeMatch(id);
      showLiveStatus(r.ok ? "✓ Match resumed." : r.unauthorized ? "⚠ Denied." : "⚠ Error resuming match.");
    } catch { showLiveStatus("⚠ Network error."); }
    finally { resumeBtn.disabled = false; }
  });

  abortBtn.addEventListener("click", async () => {
    const session = getSession();
    const id = getMatchId();
    if (!session || !id) return;
    const confirmed = window.confirm(`Abort match "${id}"?\n\nThis will immediately terminate the running match.`);
    if (!confirmed) return;
    abortBtn.disabled = true;
    try {
      const r = await session.client.abortMatch(id);
      showLiveStatus(r.ok ? "✓ Match aborted." : r.unauthorized ? "⚠ Denied." : "⚠ Error aborting match.");
      if (r.ok) {
        const idInput = document.getElementById("active-match-id") as HTMLInputElement | null;
        if (idInput) idInput.value = "";
      }
    } catch { showLiveStatus("⚠ Network error."); }
    finally { abortBtn.disabled = false; }
  });

  setTpsBtn.addEventListener("click", async () => {
    const session = getSession();
    const id = getMatchId();
    if (!session || !id) return;
    const tpsInput = document.getElementById("match-tps-input") as HTMLInputElement;
    const tps = Number(tpsInput.value);
    if (!tps || tps < 1) {
      showLiveStatus("⚠ Enter a valid TPS value (≥ 1).");
      return;
    }
    setTpsBtn.disabled = true;
    try {
      const r = await session.client.setMatchTps(id, tps);
      showLiveStatus(r.ok ? `✓ TPS set to ${tps}.` : r.unauthorized ? "⚠ Denied." : "⚠ Error setting TPS.");
    } catch { showLiveStatus("⚠ Network error."); }
    finally { setTpsBtn.disabled = false; }
  });
}

function wireExhibitionControls(): void {
  const startBtn = document.getElementById("start-exhibition-btn") as HTMLButtonElement | null;
  const stopBtn = document.getElementById("stop-exhibition-btn") as HTMLButtonElement | null;
  const refreshBtn = document.getElementById("refresh-exhibition-btn") as HTMLButtonElement | null;
  const statusEl = document.getElementById("exhibition-status") as HTMLElement | null;
  const tpsEl = document.getElementById("exhibition-tps") as HTMLInputElement;
  if (!startBtn || !stopBtn || !refreshBtn || !statusEl || !tpsEl) return;

  async function refreshStatus(): Promise<void> {
    const session = getSession();
    if (!session || !statusEl) return;
    try {
      const r = await session.client.getExhibition();
      if (r.ok) {
        statusEl.textContent = `Status: ${r.running ? "▶ Running" : "⏹ Stopped"} — ${r.matchCount} match(es) played`;
      } else if (r.unauthorized) {
        statusEl.textContent = "Status: ⚠ Denied — session may have expired.";
      } else {
        statusEl.textContent = "Status: ⚠ Could not fetch status.";
      }
    } catch {
      statusEl.textContent = "Status: ⚠ Network error.";
    }
  }

  startBtn.addEventListener("click", async () => {
    const session = getSession();
    if (!session) return;
    const tps = (tpsEl.value.trim()) ? Number(tpsEl.value) : 30;
    startBtn.disabled = true;
    try {
      const r = await session.client.startExhibition(tps);
      statusEl.textContent = r.ok ? "Status: ▶ Exhibition started." : r.unauthorized ? "Status: ⚠ Denied." : "Status: ⚠ Error starting exhibition.";
      if (r.ok) await refreshStatus();
    } catch { statusEl.textContent = "Status: ⚠ Network error."; }
    finally { startBtn.disabled = false; }
  });

  stopBtn.addEventListener("click", async () => {
    const session = getSession();
    if (!session) return;
    stopBtn.disabled = true;
    try {
      const r = await session.client.stopExhibition();
      statusEl.textContent = r.ok ? "Status: ⏹ Exhibition stopped." : r.unauthorized ? "Status: ⚠ Denied." : "Status: ⚠ Error stopping exhibition.";
      if (r.ok) await refreshStatus();
    } catch { statusEl.textContent = "Status: ⚠ Network error."; }
    finally { stopBtn.disabled = false; }
  });

  refreshBtn.addEventListener("click", refreshStatus);

  // Load initial status
  void refreshStatus();
}

// ── Ladder panel (issue 12) ───────────────────────────────────────────────────

/**
 * Inject the ladder panel into #admin-panels.
 *
 * Sections:
 *   1. Runner control — start/stop the headless ladder runner + live status.
 *   2. TrueSkill standings — sortable table from GET /ladder/standings.
 *   3. Reset ratings — POST /ladder/reset (with confirmation).
 */
function renderLadderPanel(): void {
  const container = document.getElementById("admin-panels");
  if (!container) return;

  const panel = document.createElement("section");
  panel.className = "panel";
  panel.id = "ladder-panel";
  panel.innerHTML = `
    <h2>Ladder</h2>

    <!-- 1. Runner control -->
    <div class="subpanel" id="ladder-runner-subpanel">
      <h3>Headless Runner</h3>
      <p class="hint">
        The ladder runner continuously plays headless matches to build up
        TrueSkill rankings. Start it to accumulate data; stop it to pause.
      </p>
      <div class="ladder-runner-controls">
        <button id="start-runner-btn" class="btn-primary">▶ Start Runner</button>
        <button id="stop-runner-btn" class="btn-secondary">⏹ Stop Runner</button>
        <button id="refresh-runner-btn" class="btn-secondary">↺ Refresh Status</button>
      </div>
      <p id="runner-status" class="status-line">Status: unknown</p>
    </div>

    <!-- 2. TrueSkill standings -->
    <div class="subpanel" id="standings-subpanel">
      <h3>TrueSkill Standings</h3>
      <div class="standings-controls">
        <button id="refresh-standings-btn" class="btn-secondary">↺ Refresh Standings</button>
      </div>
      <p id="standings-status" class="status-line" hidden></p>
      <div class="table-wrap">
        <table id="standings-table" hidden>
          <thead>
            <tr>
              <th>#</th>
              <th>Competitor</th>
              <th title="Conservative skill estimate: µ − 3σ">Skill (µ − 3σ)</th>
              <th title="TrueSkill mean">µ</th>
              <th title="TrueSkill sigma">σ</th>
              <th>Matches</th>
            </tr>
          </thead>
          <tbody id="standings-body"></tbody>
        </table>
      </div>
    </div>

    <!-- 3. Reset ratings -->
    <div class="subpanel" id="ladder-reset-subpanel">
      <h3>Reset Ratings</h3>
      <p class="hint">
        Resets all TrueSkill ratings to their initial values. This cannot be
        undone. Recorded matches are not deleted.
      </p>
      <button id="reset-ladder-btn" class="btn-danger">⚠ Reset All Ratings</button>
      <p id="ladder-reset-status" class="status-line" hidden></p>
    </div>
  `;

  container.appendChild(panel);

  wireLadderRunnerControls();
  wireStandingsControls();
  wireLadderResetControls();
}

function renderStandingsTable(standings: LadderStanding[]): void {
  const table = document.getElementById("standings-table");
  const statusEl = document.getElementById("standings-status");
  const tbody = document.getElementById("standings-body");
  if (!table || !statusEl || !tbody) return;

  tbody.innerHTML = "";

  if (standings.length === 0) {
    statusEl.textContent = "No standings data yet — start the ladder runner to accumulate matches.";
    statusEl.hidden = false;
    table.hidden = true;
    return;
  }

  statusEl.textContent = `${standings.length} competitor(s) — updated ${new Date().toLocaleTimeString()}`;
  statusEl.hidden = false;

  standings.forEach((s, i) => {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="col-rank">${i + 1}</td>
      <td class="col-competitor">${escHtml(s.competitor)}</td>
      <td class="col-skill">${s.conservativeSkill.toFixed(2)}</td>
      <td class="col-mu">${s.mu.toFixed(2)}</td>
      <td class="col-sigma">${s.sigma.toFixed(2)}</td>
      <td class="col-matches">${s.matches}</td>
    `;
    tbody.appendChild(tr);
  });

  table.hidden = false;
}

function wireLadderRunnerControls(): void {
  const startBtn = document.getElementById("start-runner-btn") as HTMLButtonElement | null;
  const stopBtn = document.getElementById("stop-runner-btn") as HTMLButtonElement | null;
  const refreshBtn = document.getElementById("refresh-runner-btn") as HTMLButtonElement | null;
  const statusEl = document.getElementById("runner-status") as HTMLElement | null;
  if (!startBtn || !stopBtn || !refreshBtn || !statusEl) return;

  async function refreshRunnerStatus(): Promise<void> {
    const session = getSession();
    if (!session || !statusEl) return;
    try {
      const r = await session.client.getLadderRunner();
      if (r.ok) {
        statusEl.textContent = `Status: ${r.running ? "▶ Running" : "⏹ Stopped"}`;
      } else if (r.unauthorized) {
        statusEl.textContent = "Status: ⚠ Denied — session may have expired.";
      } else {
        statusEl.textContent = "Status: ⚠ Could not fetch runner status.";
      }
    } catch {
      statusEl.textContent = "Status: ⚠ Network error.";
    }
  }

  startBtn.addEventListener("click", async () => {
    const session = getSession();
    if (!session) return;
    startBtn.disabled = true;
    try {
      const r = await session.client.startLadderRunner();
      statusEl.textContent = r.ok ? "Status: ▶ Runner started." : r.unauthorized ? "Status: ⚠ Denied." : "Status: ⚠ Error starting runner.";
      if (r.ok) await refreshRunnerStatus();
    } catch { statusEl.textContent = "Status: ⚠ Network error."; }
    finally { startBtn.disabled = false; }
  });

  stopBtn.addEventListener("click", async () => {
    const session = getSession();
    if (!session) return;
    stopBtn.disabled = true;
    try {
      const r = await session.client.stopLadderRunner();
      statusEl.textContent = r.ok ? "Status: ⏹ Runner stopped." : r.unauthorized ? "Status: ⚠ Denied." : "Status: ⚠ Error stopping runner.";
      if (r.ok) await refreshRunnerStatus();
    } catch { statusEl.textContent = "Status: ⚠ Network error."; }
    finally { stopBtn.disabled = false; }
  });

  refreshBtn.addEventListener("click", refreshRunnerStatus);
  void refreshRunnerStatus();
}

function wireStandingsControls(): void {
  const refreshBtn = document.getElementById("refresh-standings-btn") as HTMLButtonElement | null;
  if (!refreshBtn) return;

  async function loadStandings(): Promise<void> {
    const session = getSession();
    if (!session) return;
    const statusEl = document.getElementById("standings-status") as HTMLElement | null;
    refreshBtn!.disabled = true;
    try {
      const standings = await session.client.getLadderStandings();
      renderStandingsTable(standings);
    } catch {
      if (statusEl) {
        statusEl.textContent = "⚠ Failed to load standings — server returned an error.";
        statusEl.hidden = false;
      }
    } finally {
      refreshBtn!.disabled = false;
    }
  }

  refreshBtn.addEventListener("click", loadStandings);
  void loadStandings();
}

function wireLadderResetControls(): void {
  const resetBtn = document.getElementById("reset-ladder-btn") as HTMLButtonElement | null;
  const statusEl = document.getElementById("ladder-reset-status") as HTMLElement | null;
  if (!resetBtn || !statusEl) return;

  function showResetStatus(msg: string): void {
    statusEl!.textContent = msg;
    statusEl!.hidden = false;
  }

  resetBtn.addEventListener("click", async () => {
    const session = getSession();
    if (!session) return;

    const confirmed = window.confirm(
      "Reset ALL TrueSkill ratings?\n\nThis cannot be undone. Recorded matches are not deleted.",
    );
    if (!confirmed) return;

    resetBtn.disabled = true;
    try {
      const r = await session.client.resetLadder();
      if (r.unauthorized) {
        showResetStatus("⚠ Denied — session may have expired.");
      } else if (!r.ok) {
        showResetStatus("⚠ Reset failed — server returned an error.");
      } else {
        showResetStatus("✓ Ratings reset. Refresh standings to see the change.");
        // Reload standings to reflect the reset
        const standingsRefreshBtn = document.getElementById("refresh-standings-btn") as HTMLButtonElement | null;
        standingsRefreshBtn?.click();
      }
    } catch {
      showResetStatus("⚠ Network error during reset.");
    } finally {
      resetBtn.disabled = false;
    }
  });
}

// ── Replays panel (issue 12) ──────────────────────────────────────────────────

/**
 * Inject the replays panel into #admin-panels.
 *
 * Sections:
 *   1. Recordings list — GET /recordings with Replay + Download per row.
 *      Replay: POST /recordings/{id}/replay → feeds the observer hub;
 *              link to /index.html (Viewer) shown after success.
 *      Download: GET /admin/recordings/{id}/download → saves the full artifact JSON.
 *   2. Import replay — file picker → POST /admin/recordings/import → refresh list.
 */
function renderReplaysPanel(): void {
  const container = document.getElementById("admin-panels");
  if (!container) return;

  const panel = document.createElement("section");
  panel.className = "panel";
  panel.id = "replays-panel";
  panel.innerHTML = `
    <h2>Replay Management</h2>

    <div class="subpanel" id="recordings-subpanel">
      <h3>Recorded Matches</h3>
      <div class="recordings-controls">
        <button id="refresh-recordings-btn" class="btn-secondary">↺ Refresh List</button>
      </div>
      <p class="hint">
        <strong>Replay</strong> streams a recording to the observer hub — open the
        <a href="/index.html" target="_blank" rel="noopener">Viewer (/index.html)</a>
        on the projector screen to watch it.
        <strong>Download</strong> saves the full replay artifact as JSON (re-importable).
      </p>
      <p id="recordings-status" class="status-line" hidden></p>
      <div class="table-wrap">
        <table id="recordings-table" hidden>
          <thead>
            <tr>
              <th>Match ID</th>
              <th>Seed</th>
              <th>Ticks</th>
              <th>Winner</th>
              <th>Scores</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody id="recordings-body"></tbody>
        </table>
      </div>
    </div>

    <div class="subpanel" id="import-recording-subpanel">
      <h3>Import Replay</h3>
      <p class="hint">
        Load a previously downloaded <code>replay-*.json</code> artifact back into the server.
        After import the match will appear in the list above and can be replayed.
      </p>
      <div class="recordings-controls">
        <input type="file" id="import-recording-file" accept=".json" />
        <button id="import-recording-btn" class="btn-secondary">⬆ Import</button>
      </div>
      <p id="import-recording-status" class="status-line" hidden></p>
    </div>
  `;

  container.appendChild(panel);
  wireRecordingsControls();
}

function renderRecordingsTable(recordings: RecordingListItem[]): void {
  const table = document.getElementById("recordings-table");
  const statusEl = document.getElementById("recordings-status");
  const tbody = document.getElementById("recordings-body");
  if (!table || !statusEl || !tbody) return;

  tbody.innerHTML = "";

  if (recordings.length === 0) {
    statusEl.textContent = "No recordings yet — play some matches to create recordings.";
    statusEl.hidden = false;
    table.hidden = true;
    return;
  }

  statusEl.textContent = `${recordings.length} recording(s) — updated ${new Date().toLocaleTimeString()}`;
  statusEl.hidden = false;

  for (const rec of recordings) {
    const scoresText = Object.entries(rec.scores)
      .map(([team, score]) => `${escHtml(team)}: ${score}`)
      .join(", ");
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="col-match-id" title="${escHtml(rec.matchId)}">${escHtml(rec.matchId.slice(0, 8))}…</td>
      <td class="col-seed">${rec.seed}</td>
      <td class="col-ticks">${rec.tickCount}</td>
      <td class="col-winner">${rec.winner ? escHtml(rec.winner) : "<em>draw</em>"}</td>
      <td class="col-scores">${scoresText || "—"}</td>
      <td class="col-actions">
        <button class="btn-replay" data-id="${escHtml(rec.matchId)}">▶ Replay</button>
        <button class="btn-download" data-id="${escHtml(rec.matchId)}">⬇ Download</button>
      </td>
    `;
    tbody.appendChild(tr);
  }

  table.hidden = false;

  tbody.addEventListener("click", (e) => handleRecordingAction(e));
}

async function handleRecordingAction(e: Event): Promise<void> {
  const target = e.target as HTMLElement;
  const replayBtn = target.closest(".btn-replay") as HTMLButtonElement | null;
  const downloadBtn = target.closest(".btn-download") as HTMLButtonElement | null;

  if (replayBtn) {
    const id = replayBtn.dataset.id;
    if (!id) return;
    const session = getSession();
    if (!session) return;

    replayBtn.disabled = true;
    replayBtn.textContent = "Replaying…";

    try {
      const r = await session.client.replayRecording(id);
      if (r.ok) {
        replayBtn.textContent = "✓ Sent";
        // Show a convenience note — the observer hub is now streaming the replay
        const statusEl = document.getElementById("recordings-status");
        if (statusEl) {
          statusEl.textContent =
            `✓ Replay of ${id.slice(0, 8)}… sent to observer hub — open the ` +
            `Viewer at /index.html to watch.`;
          statusEl.hidden = false;
        }
      } else {
        replayBtn.textContent = "▶ Replay";
        alert(`Failed to replay recording "${id}" — server returned an error.`);
      }
    } catch {
      replayBtn.textContent = "▶ Replay";
      alert(`Network error while replaying recording "${id}".`);
    } finally {
      replayBtn.disabled = false;
    }
    return;
  }

  if (downloadBtn) {
    const id = downloadBtn.dataset.id;
    if (!id) return;
    const session = getSession();
    if (!session) return;

    downloadBtn.disabled = true;
    downloadBtn.textContent = "Downloading…";

    try {
      const r = await session.client.downloadRecording(id);
      if (r.ok && r.data) {
        // Trigger a browser file download of the full artifact JSON
        const json = JSON.stringify(r.data, null, 2);
        const blob = new Blob([json], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = `replay-${id}.json`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        downloadBtn.textContent = "✓ Downloaded";
      } else if (r.unauthorized) {
        downloadBtn.textContent = "⬇ Download";
        alert("Download denied — session may have expired. Please sign out and sign in again.");
      } else if (r.notFound) {
        downloadBtn.textContent = "⬇ Download";
        alert(`Recording "${id}" not found on the server.`);
      } else {
        downloadBtn.textContent = "⬇ Download";
        alert(`Failed to download recording "${id}" — server returned an error.`);
      }
    } catch {
      downloadBtn.textContent = "⬇ Download";
      alert(`Network error while downloading recording "${id}".`);
    } finally {
      downloadBtn.disabled = false;
    }
  }
}

function wireRecordingsControls(): void {
  const refreshBtn = document.getElementById("refresh-recordings-btn") as HTMLButtonElement | null;
  if (!refreshBtn) return;

  async function loadRecordings(): Promise<void> {
    const session = getSession();
    if (!session) return;
    const statusEl = document.getElementById("recordings-status") as HTMLElement | null;
    refreshBtn!.disabled = true;
    try {
      const recordings = await session.client.listRecordings();
      renderRecordingsTable(recordings);
    } catch {
      if (statusEl) {
        statusEl.textContent = "⚠ Failed to load recordings — server returned an error.";
        statusEl.hidden = false;
      }
    } finally {
      refreshBtn!.disabled = false;
    }
  }

  refreshBtn.addEventListener("click", loadRecordings);
  void loadRecordings();

  // ── Import replay ──────────────────────────────────────────────────────────
  const importBtn = document.getElementById("import-recording-btn") as HTMLButtonElement | null;
  const importFile = document.getElementById("import-recording-file") as HTMLInputElement | null;
  const importStatus = document.getElementById("import-recording-status") as HTMLElement | null;
  if (!importBtn || !importFile || !importStatus) return;

  importBtn.addEventListener("click", async () => {
    const file = importFile.files?.[0];
    if (!file) {
      importStatus.textContent = "⚠ Please select a replay JSON file first.";
      importStatus.hidden = false;
      return;
    }
    const session = getSession();
    if (!session) return;

    importBtn.disabled = true;
    importBtn.textContent = "Importing…";
    importStatus.hidden = true;

    try {
      const text = await file.text();
      let artifact: unknown;
      try {
        artifact = JSON.parse(text) as unknown;
      } catch {
        importStatus.textContent = "⚠ File is not valid JSON.";
        importStatus.hidden = false;
        return;
      }

      const r = await session.client.importRecording(artifact);
      if (r.ok) {
        importStatus.textContent = `✓ Imported "${file.name}" — refreshing list…`;
        importStatus.hidden = false;
        importFile.value = "";
        void loadRecordings();
      } else if (r.badRequest) {
        importStatus.textContent = "⚠ Import rejected — file is not a valid replay artifact.";
        importStatus.hidden = false;
      } else if (r.unauthorized) {
        importStatus.textContent = "⚠ Import denied — session may have expired. Please sign out and sign in again.";
        importStatus.hidden = false;
      } else {
        importStatus.textContent = "⚠ Import failed — server returned an error.";
        importStatus.hidden = false;
      }
    } catch {
      importStatus.textContent = "⚠ Network error during import.";
      importStatus.hidden = false;
    } finally {
      importBtn.disabled = false;
      importBtn.textContent = "⬆ Import";
    }
  });
}

// ── Bootstrap ─────────────────────────────────────────────────────────────────

function getApp(): HTMLElement {
  let app = document.getElementById("app");
  if (!app) {
    app = document.createElement("div");
    app.id = "app";
    document.body.appendChild(app);
  }
  return app;
}

try {
  const pw = getStoredPassword()
  if (pw !== null) {
    const client = createAdminClient(BASE_URL, pw);
    const result = await client.verifyAuth();
    if (!result.ok) {
      const msg = result.unauthorized
        ? "Stall password cleared — access denied."
        : "Could not reach the server. Cleared stored password";
      clearSession();
      renderSignIn(msg);
    } else {
      setSession(pw, client);
      renderDashboard();
    }
  } else {
    renderSignIn();
  }
} catch {
  clearSession();
  renderSignIn("Network error — could not connect to the server.");
}
