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
            </tr>
          </thead>
          <tbody id="bots-body"></tbody>
        </table>
      </div>
    </section>

    <!-- Seam: issues 09-12 inject their panels here -->
    <div id="admin-panels"></div>
  `;

  document.getElementById("signout-btn")!.addEventListener("click", () => {
    stopPolling();
    clearSession();
    renderSignIn();
  });

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
    `;
    tbody.appendChild(tr);
  }

  table.hidden = false;
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
