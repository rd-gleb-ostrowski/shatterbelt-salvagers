/**
 * adminClient — typed, testable API client for the admin-gated controller API.
 *
 * All endpoints require `Authorization: Facilitator <password>`.
 *
 * Exports:
 *   BotHealthSnapshot          — mirrors server `health.rs` camelCase shape
 *   AuthResult                 — result of verifyAuth()
 *   HttpResponse               — minimal interface for the injected fetch function
 *   FetchFn                    — injectable fetch-like type (enables unit tests)
 *   AdminClient                — typed client interface
 *   createAdminClient(...)     — factory; pass a fake FetchFn in tests
 *
 * Seam: issues 09-12 call `createAdminClient` with the password held in
 * `session.ts` and the same FetchFn injection pattern.
 */

// ── Types ─────────────────────────────────────────────────────────────────────

/**
 * One bot's health, as returned by `GET /admin/bots`.
 *
 * Field names mirror the server's `#[serde(rename_all = "camelCase")]`
 * serialisation of `BotHealthSnapshot` in health.rs.
 */
export interface BotHealthSnapshot {
  /** Team name. */
  team: string;
  /** Driver kind: `"ws"`, `"wasm"`, or `"default"`. */
  kind: string;
  /** `true` while the bot is connected / active. */
  connected: boolean;
  /** Unix timestamp (ms) of last successful tick response, or `null`. */
  lastSeen: number | null;
  /** Ticks where no intent was produced (deadline miss / fuel exhaustion). */
  skippedTicks: number;
  /** Total fault count (fuel exhaustions + non-fuel WASM traps). */
  crashes: number;
  /** Recent log output captured from the bot. */
  recentLogs: string;
}

/** Result of `verifyAuth()`. Never throws for HTTP-level errors. */
export interface AuthResult {
  /** `true` when the server accepted the password. */
  ok: boolean;
  /** `true` when the server responded with 401 (wrong / absent password). */
  unauthorized: boolean;
}

/** Result of `kickBot()`. Never throws for HTTP-level errors. */
export interface KickResult {
  /** `true` when the server accepted the kick (200). */
  ok: boolean;
  /** `true` when the server responded with 401 (wrong / absent password). */
  unauthorized: boolean;
}

/**
 * Result of `uploadTeamBot()` or `setDefaultBot()`. Never throws for HTTP-level errors.
 *
 * Exactly one field is `true` on any given response; all are `false` on an
 * unexpected status (server error, network oddity, etc.).
 */
export interface UploadResult {
  /** `true` when the server accepted the upload (200). */
  ok: boolean;
  /** `true` when the server rejected the body as invalid WASM (400). */
  badRequest: boolean;
  /** `true` when the server responded with 401 (wrong / absent password). */
  unauthorized: boolean;
}

/**
 * Result of `disableBot()`, `enableBot()`, or `clearDefaultBot()`.
 * Mirrors `KickResult` — the same 200/401/other mapping.
 * Never throws for HTTP-level errors.
 */
export type BotActionResult = KickResult;

/**
 * Minimal fetch-like interface accepted by `createAdminClient`.
 *
 * Deliberately narrower than `typeof fetch` so tests can pass a simple
 * plain-object fake without constructing a real `Response`.
 */
export interface HttpResponse {
  status: number;
  json(): Promise<unknown>;
}

export type FetchFn = (url: string, init?: RequestInit) => Promise<HttpResponse>;

/** The typed admin API client returned by `createAdminClient`. */
export interface AdminClient {
  /**
   * Probe `GET /admin/bots` to verify the password.
   *
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other HTTP errors.
   * Never throws for HTTP-level failures (network errors still throw).
   */
  verifyAuth(): Promise<AuthResult>;

  /**
   * Fetch `GET /admin/bots`.
   *
   * Throws if the response is not 200 (caller should catch for display).
   * Returns the parsed `BotHealthSnapshot[]` on success.
   */
  getBots(): Promise<BotHealthSnapshot[]>;

  /**
   * Kick (disqualify) a bot via `POST /admin/bots/{team}/kick`.
   *
   * The team name is URL-encoded. No request body is sent.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other HTTP errors.
   * Never throws for HTTP-level failures (network errors still throw).
   */
  kickBot(team: string): Promise<KickResult>;

  /**
   * Upload or replace a team's WASM Bot via `POST /admin/bots/{team}`.
   *
   * The team name is URL-encoded. The WASM bytes are sent as the raw body with
   * `Content-Type: application/octet-stream`.
   *
   * Returns `{ ok: true,  badRequest: false, unauthorized: false }` on 200.
   * Returns `{ ok: false, badRequest: true,  unauthorized: false }` on 400 (invalid WASM).
   * Returns `{ ok: false, badRequest: false, unauthorized: true  }` on 401.
   * Returns `{ ok: false, badRequest: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  uploadTeamBot(team: string, wasm: ArrayBuffer | Uint8Array): Promise<UploadResult>;

  /**
   * Disable a team's bot via `POST /admin/bots/{team}/disable`.
   *
   * The team name is URL-encoded. No request body is sent.
   * The team's slot falls back to the Default Bot until re-enabled.
   *
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  disableBot(team: string): Promise<BotActionResult>;

  /**
   * Re-enable a team's bot via `POST /admin/bots/{team}/enable`.
   *
   * The team name is URL-encoded. No request body is sent.
   * Restores normal WS → WASM → Default resolution for the team.
   *
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  enableBot(team: string): Promise<BotActionResult>;

  /**
   * Set or replace the custom Default Bot via `POST /admin/default-bot`.
   *
   * The WASM bytes are sent as the raw body with
   * `Content-Type: application/octet-stream`.
   *
   * Returns `{ ok: true,  badRequest: false, unauthorized: false }` on 200.
   * Returns `{ ok: false, badRequest: true,  unauthorized: false }` on 400 (invalid WASM).
   * Returns `{ ok: false, badRequest: false, unauthorized: true  }` on 401.
   * Returns `{ ok: false, badRequest: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  setDefaultBot(wasm: ArrayBuffer | Uint8Array): Promise<UploadResult>;

  /**
   * Clear the custom Default Bot via `DELETE /admin/default-bot`.
   *
   * Reverts the Default Bot slot to the built-in heuristic driver.
   * No request body is sent.
   *
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  clearDefaultBot(): Promise<BotActionResult>;
}

// ── Factory ───────────────────────────────────────────────────────────────────

/**
 * Create a typed admin API client.
 *
 * @param baseUrl  Server origin (e.g. `""` for same-origin, or
 *                 `"http://localhost:3000"` in dev). No trailing slash.
 * @param password The facilitator password, attached as
 *                 `Authorization: Facilitator <password>` on every request.
 * @param fetchFn  Injectable fetch function. Defaults to the global `fetch`.
 *                 Pass a fake in unit tests to capture and assert requests.
 */
export function createAdminClient(
  baseUrl: string,
  password: string,
  fetchFn: FetchFn = fetch as unknown as FetchFn,
): AdminClient {
  const authHeaders: Record<string, string> = {
    Authorization: `Facilitator ${password}`,
  };

  async function get(path: string): Promise<HttpResponse> {
    return fetchFn(`${baseUrl}${path}`, {
      method: "GET",
      headers: authHeaders,
    });
  }

  async function post(path: string): Promise<HttpResponse> {
    return fetchFn(`${baseUrl}${path}`, {
      method: "POST",
      headers: authHeaders,
    });
  }

  async function postBinary(
    path: string,
    body: ArrayBuffer | Uint8Array,
  ): Promise<HttpResponse> {
    return fetchFn(`${baseUrl}${path}`, {
      method: "POST",
      headers: {
        ...authHeaders,
        "Content-Type": "application/octet-stream",
      },
      body: body as BodyInit,
    });
  }

  async function del(path: string): Promise<HttpResponse> {
    return fetchFn(`${baseUrl}${path}`, {
      method: "DELETE",
      headers: authHeaders,
    });
  }

  function mapUpload(status: number): UploadResult {
    if (status === 200) return { ok: true, badRequest: false, unauthorized: false };
    if (status === 400) return { ok: false, badRequest: true, unauthorized: false };
    if (status === 401) return { ok: false, badRequest: false, unauthorized: true };
    return { ok: false, badRequest: false, unauthorized: false };
  }

  function mapAction(status: number): BotActionResult {
    if (status === 200) return { ok: true, unauthorized: false };
    if (status === 401) return { ok: false, unauthorized: true };
    return { ok: false, unauthorized: false };
  }

  return {
    async verifyAuth(): Promise<AuthResult> {
      const res = await get("/admin/bots");
      if (res.status === 200) return { ok: true, unauthorized: false };
      if (res.status === 401) return { ok: false, unauthorized: true };
      return { ok: false, unauthorized: false };
    },

    async getBots(): Promise<BotHealthSnapshot[]> {
      const res = await get("/admin/bots");
      if (res.status !== 200) {
        throw new Error(`GET /admin/bots returned ${res.status}`);
      }
      const data = await res.json();
      return data as BotHealthSnapshot[];
    },

    async kickBot(team: string): Promise<KickResult> {
      const path = `/admin/bots/${encodeURIComponent(team)}/kick`;
      const res = await post(path);
      if (res.status === 200) return { ok: true, unauthorized: false };
      if (res.status === 401) return { ok: false, unauthorized: true };
      return { ok: false, unauthorized: false };
    },

    async uploadTeamBot(
      team: string,
      wasm: ArrayBuffer | Uint8Array,
    ): Promise<UploadResult> {
      const path = `/admin/bots/${encodeURIComponent(team)}`;
      const res = await postBinary(path, wasm);
      return mapUpload(res.status);
    },

    async disableBot(team: string): Promise<BotActionResult> {
      const path = `/admin/bots/${encodeURIComponent(team)}/disable`;
      const res = await post(path);
      return mapAction(res.status);
    },

    async enableBot(team: string): Promise<BotActionResult> {
      const path = `/admin/bots/${encodeURIComponent(team)}/enable`;
      const res = await post(path);
      return mapAction(res.status);
    },

    async setDefaultBot(wasm: ArrayBuffer | Uint8Array): Promise<UploadResult> {
      const res = await postBinary("/admin/default-bot", wasm);
      return mapUpload(res.status);
    },

    async clearDefaultBot(): Promise<BotActionResult> {
      const res = await del("/admin/default-bot");
      return mapAction(res.status);
    },
  };
}
