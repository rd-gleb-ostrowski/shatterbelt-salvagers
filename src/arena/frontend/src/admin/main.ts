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
import { setSession, getSession, clearSession } from "./session.ts";
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

renderSignIn();
