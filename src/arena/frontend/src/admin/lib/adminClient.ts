/**
 * adminClient — typed, testable API client for the admin-gated controller API.
 *
 * All endpoints require `Authorization: Facilitator <password>`.
 *
 * Exports:
 *   BotHealthSnapshot          — mirrors server `health.rs` camelCase shape
 *   AuthResult                 — result of verifyAuth()
 *   LadderStanding             — one entry from GET /ladder/standings
 *   LadderRunnerResult         — result of getLadderRunner()
 *   RecordingListItem          — one entry from GET /recordings
 *   DownloadDTO                — payload from GET /admin/recordings/{id}/download
 *   DownloadRecordingResult    — result of downloadRecording()
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
 * Options for `startMatch()`. Mirrors `StartMatchRequest` in admin.rs
 * (`#[serde(rename_all = "camelCase")]`).
 * Optional fields are omitted from the JSON body if not provided.
 */
export interface StartMatchOptions {
  /** `"live"` for a real-time observable match; `"headless"` for instant. */
  mode: "live" | "headless";
  /** RNG seed. Server uses its configured default when absent. */
  seed?: number;
  /** Maximum tick count. Server uses its configured default when absent. */
  maxTicks?: number;
  /** Team names. Server uses the full registered field when absent. */
  teams?: string[];
  /** Ticks-per-second for live matches. Server defaults to 30. */
  tps?: number;
}

/**
 * Result of `startMatch()`.
 * Exactly one discriminant is `true`; `matchId` + `mode` are set on success.
 */
export interface StartMatchResult {
  /** `true` when the server accepted the request (200). */
  ok: boolean;
  /** Assigned match identifier (populated when `ok` is `true`). */
  matchId: string;
  /** Match mode echoed by the server (`"live"` or `"headless"`). */
  mode: string;
  /** `true` when the server responded with 401 (wrong / absent password). */
  unauthorized: boolean;
}

/**
 * Result of a simple match control action (pause / resume / abort).
 * Mirrors `KickResult` — 200/401/other mapping. Never throws.
 */
export type MatchActionResult = KickResult;

/**
 * Result of `setMatchTps()`. Mirrors `KickResult`. Never throws.
 */
export type SetTpsResult = KickResult;

/**
 * Result of `getExhibition()`.
 * Exactly one discriminant is `true`; `running` + `matchCount` set on success.
 */
export interface ExhibitionResult {
  /** `true` when the server returned 200. */
  ok: boolean;
  /** Whether the exhibition loop is currently running. */
  running: boolean;
  /** Total matches played in the current exhibition run. */
  matchCount: number;
  /** `true` when the server responded with 401. */
  unauthorized: boolean;
}

/**
 * Result of `startExhibition()` or `stopExhibition()`.
 * Mirrors `KickResult` — 200/401/other mapping. Never throws.
 */
export type ExhibitionActionResult = KickResult;

// ── Issue 12: Ladder & Replay types ──────────────────────────────────────────

/**
 * One competitor's TrueSkill standing, as returned by `GET /ladder/standings`.
 * Ordered by `conservativeSkill` descending on the server.
 */
export interface LadderStanding {
  /** Team / competitor name. */
  competitor: string;
  /** TrueSkill µ (mean). */
  mu: number;
  /** TrueSkill σ (standard deviation). */
  sigma: number;
  /** Conservative skill estimate: µ − 3σ. */
  conservativeSkill: number;
  /** Total ladder matches played. */
  matches: number;
}

/**
 * Result of `getLadderRunner()`.
 * Exactly one discriminant is `true`; `running` is set on success.
 */
export interface LadderRunnerResult {
  /** `true` when the server returned 200. */
  ok: boolean;
  /** Whether the background headless ladder runner is active. */
  running: boolean;
  /** `true` when the server responded with 401. */
  unauthorized: boolean;
}

/**
 * Result of a simple ladder control action (reset / start runner / stop runner).
 * Mirrors `KickResult` — 200/401/other mapping. Never throws.
 */
export type LadderActionResult = KickResult;

/**
 * One recorded match entry, as returned by `GET /recordings`.
 * Field names mirror the server's `RecordingListItem` camelCase serialisation.
 */
export interface RecordingListItem {
  /** Unique match identifier. */
  matchId: string;
  /** RNG seed used for the match. */
  seed: number;
  /** Total tick count of the recorded match. */
  tickCount: number;
  /** Winning competitor name, or `null` if the match was a draw. */
  winner: string | null;
  /** Final score per competitor (team → score). */
  scores: Record<string, number>;
}

/**
 * Metadata DTO returned by `GET /admin/recordings/{id}/download`.
 * Mirrors the server's JSON response shape.
 */
export interface DownloadDTO {
  matchId: string;
  seed: number;
  tickCount: number;
  winner: string | null;
  scores: Record<string, number>;
}

/**
 * Result of `downloadRecording()`.
 * Exactly one discriminant is `true`; `data` is populated on success.
 */
export interface DownloadRecordingResult {
  /** `true` when the server returned 200 and data was parsed. */
  ok: boolean;
  /** The parsed DTO on success; `null` otherwise. */
  data: DownloadDTO | null;
  /** `true` when the server responded with 401. */
  unauthorized: boolean;
  /** `true` when the server responded with 404 (recording not found). */
  notFound: boolean;
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

  // ── Issue 11: match control & push-to-projector ──────────────────────────

  /**
   * Start an on-demand match via `POST /admin/matches`.
   *
   * JSON body: `{ mode, seed?, maxTicks?, teams?, tps? }` (camelCase).
   * Optional fields are omitted when not provided (not sent as `null`).
   * `Content-Type: application/json` is set automatically.
   *
   * On 200 parses `{ matchId, mode }` from the response.
   * Returns `{ ok: true, matchId, mode, unauthorized: false }` on 200.
   * Returns `{ ok: false, matchId: "", mode: "", unauthorized: true }` on 401.
   * Returns `{ ok: false, matchId: "", mode: "", unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   *
   * Push-to-projector note: starting a `"live"` match automatically feeds the
   * observer hub at `/observe`, which is the endpoint the Viewer (projector)
   * connects to. There is no separate "push" endpoint — a live match IS the
   * projector feed.
   */
  startMatch(opts: StartMatchOptions): Promise<StartMatchResult>;

  /**
   * Pause a running live match via `POST /admin/matches/{id}/pause`.
   *
   * The match id is URL-encoded. No request body is sent.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  pauseMatch(id: string): Promise<MatchActionResult>;

  /**
   * Resume a paused live match via `POST /admin/matches/{id}/resume`.
   *
   * The match id is URL-encoded. No request body is sent.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  resumeMatch(id: string): Promise<MatchActionResult>;

  /**
   * Abort (delete) a live match via `DELETE /admin/matches/{id}`.
   *
   * The match id is URL-encoded. No request body is sent.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  abortMatch(id: string): Promise<MatchActionResult>;

  /**
   * Change the tick rate of a live match via `POST /admin/matches/{id}/tps`.
   *
   * JSON body: `{ tps: number }`.
   * The match id is URL-encoded.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  setMatchTps(id: string, tps: number): Promise<SetTpsResult>;

  /**
   * Get exhibition status via `GET /admin/exhibition`.
   *
   * Returns `{ ok: true, running, matchCount, unauthorized: false }` on 200.
   * Returns `{ ok: false, running: false, matchCount: 0, unauthorized: true }` on 401.
   * Returns `{ ok: false, running: false, matchCount: 0, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  getExhibition(): Promise<ExhibitionResult>;

  /**
   * Start the exhibition loop via `POST /admin/exhibition/start`.
   *
   * No request body is sent.
   * The exhibition loop continuously runs live matches, keeping the projector
   * Viewer (at `/observe`) active whenever no on-demand match is running.
   *
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  startExhibition(): Promise<ExhibitionActionResult>;

  /**
   * Stop the exhibition loop via `POST /admin/exhibition/stop`.
   *
   * No request body is sent.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  stopExhibition(): Promise<ExhibitionActionResult>;

  // ── Issue 12: Ladder control & replay management ─────────────────────────

  /**
   * Fetch `GET /ladder/standings` (public endpoint).
   *
   * Throws if the response is not 200. Returns the parsed `LadderStanding[]`
   * ordered by conservativeSkill descending on the server.
   */
  getLadderStandings(): Promise<LadderStanding[]>;

  /**
   * Reset all TrueSkill ratings via `POST /ladder/reset` (facilitator-gated).
   *
   * No request body is sent.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  resetLadder(): Promise<LadderActionResult>;

  /**
   * Get ladder runner status via `GET /admin/ladder/runner` (facilitator-gated).
   *
   * Returns `{ ok: true, running, unauthorized: false }` on 200.
   * Returns `{ ok: false, running: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, running: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  getLadderRunner(): Promise<LadderRunnerResult>;

  /**
   * Start the background headless ladder runner via `POST /admin/ladder/runner/start`.
   *
   * Idempotent — safe to call when already running.
   * No request body is sent.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  startLadderRunner(): Promise<LadderActionResult>;

  /**
   * Stop the background headless ladder runner via `POST /admin/ladder/runner/stop`.
   *
   * No request body is sent.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: true }` on 401.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  stopLadderRunner(): Promise<LadderActionResult>;

  /**
   * List all recorded matches via `GET /recordings` (public endpoint).
   *
   * Throws if the response is not 200. Returns the parsed `RecordingListItem[]`.
   */
  listRecordings(): Promise<RecordingListItem[]>;

  /**
   * Replay a recording through the observer hub via `POST /recordings/{id}/replay`.
   *
   * The id is URL-encoded. The Viewer at `/observe` will show the replay.
   * Returns `{ ok: true, unauthorized: false }` on 200.
   * Returns `{ ok: false, unauthorized: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  replayRecording(id: string): Promise<LadderActionResult>;

  /**
   * Download recording metadata via `GET /admin/recordings/{id}/download`.
   *
   * The id is URL-encoded.
   * Returns `{ ok: true, data: DownloadDTO, unauthorized: false, notFound: false }` on 200.
   * Returns `{ ok: false, data: null, unauthorized: true,  notFound: false }` on 401.
   * Returns `{ ok: false, data: null, unauthorized: false, notFound: true  }` on 404.
   * Returns `{ ok: false, data: null, unauthorized: false, notFound: false }` on other errors.
   * Never throws for HTTP-level failures.
   */
  downloadRecording(id: string): Promise<DownloadRecordingResult>;
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

  async function postJson(path: string, body: Record<string, unknown>): Promise<HttpResponse> {
    return fetchFn(`${baseUrl}${path}`, {
      method: "POST",
      headers: {
        ...authHeaders,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
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

    // ── Issue 11: match control & push-to-projector ──────────────────────────

    async startMatch(opts: StartMatchOptions): Promise<StartMatchResult> {
      const body: Record<string, unknown> = { mode: opts.mode };
      if (opts.seed !== undefined) body.seed = opts.seed;
      if (opts.maxTicks !== undefined) body.maxTicks = opts.maxTicks;
      if (opts.teams !== undefined) body.teams = opts.teams;
      if (opts.tps !== undefined) body.tps = opts.tps;

      const res = await postJson("/admin/matches", body);
      if (res.status === 200) {
        const data = (await res.json()) as { matchId: string; mode: string };
        return { ok: true, matchId: data.matchId, mode: data.mode, unauthorized: false };
      }
      if (res.status === 401) {
        return { ok: false, matchId: "", mode: "", unauthorized: true };
      }
      return { ok: false, matchId: "", mode: "", unauthorized: false };
    },

    async pauseMatch(id: string): Promise<MatchActionResult> {
      const path = `/admin/matches/${encodeURIComponent(id)}/pause`;
      const res = await post(path);
      return mapAction(res.status);
    },

    async resumeMatch(id: string): Promise<MatchActionResult> {
      const path = `/admin/matches/${encodeURIComponent(id)}/resume`;
      const res = await post(path);
      return mapAction(res.status);
    },

    async abortMatch(id: string): Promise<MatchActionResult> {
      const path = `/admin/matches/${encodeURIComponent(id)}`;
      const res = await del(path);
      return mapAction(res.status);
    },

    async setMatchTps(id: string, tps: number): Promise<SetTpsResult> {
      const path = `/admin/matches/${encodeURIComponent(id)}/tps`;
      const res = await postJson(path, { tps });
      return mapAction(res.status);
    },

    async getExhibition(): Promise<ExhibitionResult> {
      const res = await get("/admin/exhibition");
      if (res.status === 200) {
        const data = (await res.json()) as { running: boolean; matchCount: number };
        return { ok: true, running: data.running, matchCount: data.matchCount, unauthorized: false };
      }
      if (res.status === 401) {
        return { ok: false, running: false, matchCount: 0, unauthorized: true };
      }
      return { ok: false, running: false, matchCount: 0, unauthorized: false };
    },

    async startExhibition(): Promise<ExhibitionActionResult> {
      const res = await post("/admin/exhibition/start");
      return mapAction(res.status);
    },

    async stopExhibition(): Promise<ExhibitionActionResult> {
      const res = await post("/admin/exhibition/stop");
      return mapAction(res.status);
    },

    // ── Issue 12: Ladder control & replay management ────────────────────────

    async getLadderStandings(): Promise<LadderStanding[]> {
      const res = await get("/ladder/standings");
      if (res.status !== 200) {
        throw new Error(`GET /ladder/standings returned ${res.status}`);
      }
      const data = await res.json();
      return data as LadderStanding[];
    },

    async resetLadder(): Promise<LadderActionResult> {
      const res = await post("/ladder/reset");
      return mapAction(res.status);
    },

    async getLadderRunner(): Promise<LadderRunnerResult> {
      const res = await get("/admin/ladder/runner");
      if (res.status === 200) {
        const data = (await res.json()) as { running: boolean };
        return { ok: true, running: data.running, unauthorized: false };
      }
      if (res.status === 401) {
        return { ok: false, running: false, unauthorized: true };
      }
      return { ok: false, running: false, unauthorized: false };
    },

    async startLadderRunner(): Promise<LadderActionResult> {
      const res = await post("/admin/ladder/runner/start");
      return mapAction(res.status);
    },

    async stopLadderRunner(): Promise<LadderActionResult> {
      const res = await post("/admin/ladder/runner/stop");
      return mapAction(res.status);
    },

    async listRecordings(): Promise<RecordingListItem[]> {
      const res = await get("/recordings");
      if (res.status !== 200) {
        throw new Error(`GET /recordings returned ${res.status}`);
      }
      const data = await res.json();
      return data as RecordingListItem[];
    },

    async replayRecording(id: string): Promise<LadderActionResult> {
      const path = `/recordings/${encodeURIComponent(id)}/replay`;
      const res = await post(path);
      return mapAction(res.status);
    },

    async downloadRecording(id: string): Promise<DownloadRecordingResult> {
      const path = `/admin/recordings/${encodeURIComponent(id)}/download`;
      const res = await get(path);
      if (res.status === 200) {
        const data = (await res.json()) as DownloadDTO;
        return { ok: true, data, unauthorized: false, notFound: false };
      }
      if (res.status === 401) {
        return { ok: false, data: null, unauthorized: true, notFound: false };
      }
      if (res.status === 404) {
        return { ok: false, data: null, unauthorized: false, notFound: true };
      }
      return { ok: false, data: null, unauthorized: false, notFound: false };
    },
  };
}
