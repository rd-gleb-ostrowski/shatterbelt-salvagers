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
  };
}
