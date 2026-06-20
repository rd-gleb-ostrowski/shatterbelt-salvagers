/**
 * Unit tests for adminClient — pure logic (request building + response parsing).
 *
 * Covers:
 *   Slice 1 — Authorization header is attached to every request
 *   Slice 2 — getBots parses a JSON array into typed BotHealthSnapshot[]
 *   Slice 3 — verifyAuth returns { ok: false, unauthorized: true } on 401
 *   Slice 4 — verifyAuth returns { ok: true, unauthorized: false } on 200
 *   Slice 5 — getBots surfaces non-200 as a thrown error (not silent)
 *   Slice 6 — empty bots array is handled gracefully by getBots
 *   Slice 7 — verifyAuth returns ok:false, unauthorized:false on other errors
 *
 * DOM / live-fetch / polling are manual/visual only (PRD Testing Decisions).
 */

import { describe, it, expect } from "vitest";
import {
  createAdminClient,
  type BotHealthSnapshot,
  type FetchFn,
  type LadderStanding,
  type RecordingListItem,
  type RecordingArtifact,
} from "./adminClient.ts";

// ── Helpers ───────────────────────────────────────────────────────────────────

interface CapturedRequest {
  url: string;
  init?: RequestInit;
}

function makeFakeFetch(
  status: number,
  body: unknown,
): { fetchFn: FetchFn; captured: CapturedRequest[] } {
  const captured: CapturedRequest[] = [];
  const fetchFn: FetchFn = async (url, init) => {
    captured.push({ url, init });
    return {
      status,
      async json() {
        return body;
      },
    };
  };
  return { fetchFn, captured };
}

const BASE = "http://localhost:3000";
const PASSWORD = "s3cret";

const SAMPLE_BOTS: BotHealthSnapshot[] = [
  {
    team: "alpha",
    kind: "ws",
    connected: true,
    lastSeen: 1_700_000_000_000,
    skippedTicks: 2,
    crashes: 0,
    recentLogs: "hello",
  },
  {
    team: "beta",
    kind: "wasm",
    connected: false,
    lastSeen: null,
    skippedTicks: 10,
    crashes: 3,
    recentLogs: "",
  },
];

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("createAdminClient", () => {
  // Slice 1 — Authorization header attached
  describe("authorization header", () => {
    it("attaches Authorization: Facilitator <password> to getBots request", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, SAMPLE_BOTS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getBots();
      expect(captured).toHaveLength(1);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("attaches Authorization: Facilitator <password> to verifyAuth request", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, []);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.verifyAuth();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("uses the correct password verbatim (no trimming by client)", async () => {
      const pw = "my special pw 123";
      const { fetchFn, captured } = makeFakeFetch(200, []);
      const client = createAdminClient(BASE, pw, fetchFn);
      await client.verifyAuth();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${pw}`);
    });

    it("requests the correct path GET /admin/bots", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, []);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getBots();
      expect(captured[0].url).toBe(`${BASE}/admin/bots`);
    });

    it("uses GET method", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, []);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getBots();
      expect(captured[0].init?.method).toBe("GET");
    });
  });

  // Slice 2 — getBots parses BotHealthSnapshot[]
  describe("getBots response parsing", () => {
    it("returns a typed BotHealthSnapshot[] on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, SAMPLE_BOTS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const bots = await client.getBots();
      expect(bots).toHaveLength(2);
      expect(bots[0].team).toBe("alpha");
      expect(bots[0].kind).toBe("ws");
      expect(bots[0].connected).toBe(true);
      expect(bots[0].lastSeen).toBe(1_700_000_000_000);
      expect(bots[0].skippedTicks).toBe(2);
      expect(bots[0].crashes).toBe(0);
      expect(bots[0].recentLogs).toBe("hello");
    });

    it("preserves null lastSeen from the server", async () => {
      const { fetchFn } = makeFakeFetch(200, SAMPLE_BOTS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const bots = await client.getBots();
      expect(bots[1].lastSeen).toBeNull();
    });

    it("preserves all numeric fields from the server", async () => {
      const { fetchFn } = makeFakeFetch(200, SAMPLE_BOTS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const bots = await client.getBots();
      expect(bots[1].skippedTicks).toBe(10);
      expect(bots[1].crashes).toBe(3);
    });
  });

  // Slice 3 — 401 → verifyAuth unauthorized
  describe("verifyAuth on 401", () => {
    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.verifyAuth();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("does not throw on 401 (returns gracefully)", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.verifyAuth()).resolves.not.toThrow();
    });
  });

  // Slice 4 — 200 → verifyAuth ok
  describe("verifyAuth on 200", () => {
    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, []);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.verifyAuth();
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });
  });

  // Slice 5 — non-200 in getBots throws
  describe("getBots error handling", () => {
    it("throws on non-200 response (e.g. 401)", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.getBots()).rejects.toThrow();
    });

    it("throws on 500 response", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.getBots()).rejects.toThrow("500");
    });
  });

  // Slice 6 — empty bots array
  describe("getBots with empty array", () => {
    it("returns an empty array when server returns []", async () => {
      const { fetchFn } = makeFakeFetch(200, []);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const bots = await client.getBots();
      expect(bots).toEqual([]);
    });
  });

  // Slice 7 — other HTTP errors in verifyAuth
  describe("verifyAuth on other HTTP errors", () => {
    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.verifyAuth();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: false } on 403", async () => {
      const { fetchFn } = makeFakeFetch(403, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.verifyAuth();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });
  });

  // ── kickBot slices ────────────────────────────────────────────────────────

  // Slice 8 — kickBot builds POST to correct path with auth header
  describe("kickBot request building", () => {
    it("sends a POST request (not GET)", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.kickBot("alpha");
      expect(captured[0].init?.method).toBe("POST");
    });

    it("sends to /admin/bots/{team}/kick", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.kickBot("alpha");
      expect(captured[0].url).toBe(`${BASE}/admin/bots/alpha/kick`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.kickBot("alpha");
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.kickBot("alpha");
      expect(captured[0].init?.body).toBeUndefined();
    });
  });

  // Slice 9 — kickBot URL-encodes team names with special chars
  describe("kickBot URL encoding", () => {
    it("URL-encodes a team name containing a space", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.kickBot("team a");
      expect(captured[0].url).toBe(`${BASE}/admin/bots/team%20a/kick`);
    });

    it("URL-encodes a team name containing a slash", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.kickBot("team/x");
      expect(captured[0].url).toBe(`${BASE}/admin/bots/team%2Fx/kick`);
    });
  });

  // Slice 10 — kickBot 200 → ok
  describe("kickBot on 200", () => {
    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.kickBot("alpha");
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.kickBot("alpha")).resolves.not.toThrow();
    });
  });

  // Slice 11 — kickBot 401 → unauthorized (no throw)
  describe("kickBot on 401", () => {
    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.kickBot("alpha");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("does not throw on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.kickBot("alpha")).resolves.not.toThrow();
    });
  });

  // Slice 12 — kickBot non-200/401 → error result (no throw)
  describe("kickBot on other HTTP errors", () => {
    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.kickBot("alpha");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: false } on 403", async () => {
      const { fetchFn } = makeFakeFetch(403, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.kickBot("alpha");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on non-200/401 response", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.kickBot("alpha")).resolves.not.toThrow();
    });
  });

  // ── uploadTeamBot slices ──────────────────────────────────────────────────

  const SAMPLE_WASM = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00]);

  // Slice 13 — uploadTeamBot builds POST to correct path with auth header + binary body
  describe("uploadTeamBot request building", () => {
    it("sends a POST request (not GET)", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.uploadTeamBot("alpha", SAMPLE_WASM);
      expect(captured[0].init?.method).toBe("POST");
    });

    it("sends to /admin/bots/{team}", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.uploadTeamBot("alpha", SAMPLE_WASM);
      expect(captured[0].url).toBe(`${BASE}/admin/bots/alpha`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.uploadTeamBot("alpha", SAMPLE_WASM);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("passes the binary body through to fetch", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.uploadTeamBot("alpha", SAMPLE_WASM);
      expect(captured[0].init?.body).toBe(SAMPLE_WASM);
    });

    it("sets Content-Type: application/octet-stream", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.uploadTeamBot("alpha", SAMPLE_WASM);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Content-Type"]).toBe("application/octet-stream");
    });
  });

  // Slice 14 — uploadTeamBot response mapping (200/400/401/other; no throws)
  describe("uploadTeamBot response mapping", () => {
    it("returns { ok: true, badRequest: false, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.uploadTeamBot("alpha", SAMPLE_WASM);
      expect(result).toEqual({ ok: true, badRequest: false, unauthorized: false });
    });

    it("returns { ok: false, badRequest: true, unauthorized: false } on 400", async () => {
      const { fetchFn } = makeFakeFetch(400, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.uploadTeamBot("alpha", SAMPLE_WASM);
      expect(result).toEqual({ ok: false, badRequest: true, unauthorized: false });
    });

    it("returns { ok: false, badRequest: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.uploadTeamBot("alpha", SAMPLE_WASM);
      expect(result).toEqual({ ok: false, badRequest: false, unauthorized: true });
    });

    it("returns { ok: false, badRequest: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.uploadTeamBot("alpha", SAMPLE_WASM);
      expect(result).toEqual({ ok: false, badRequest: false, unauthorized: false });
    });

    it("does not throw on 400 (bad WASM)", async () => {
      const { fetchFn } = makeFakeFetch(400, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.uploadTeamBot("alpha", SAMPLE_WASM)).resolves.not.toThrow();
    });

    it("does not throw on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.uploadTeamBot("alpha", SAMPLE_WASM)).resolves.not.toThrow();
    });
  });

  // Slice 15 — disableBot builds POST to correct path, no body
  describe("disableBot request building", () => {
    it("sends a POST request", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.disableBot("alpha");
      expect(captured[0].init?.method).toBe("POST");
    });

    it("sends to /admin/bots/{team}/disable", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.disableBot("alpha");
      expect(captured[0].url).toBe(`${BASE}/admin/bots/alpha/disable`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.disableBot("alpha");
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.disableBot("alpha");
      expect(captured[0].init?.body).toBeUndefined();
    });
  });

  // Slice 16 — disableBot response mapping
  describe("disableBot response mapping", () => {
    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.disableBot("alpha");
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.disableBot("alpha");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.disableBot("alpha");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.disableBot("alpha")).resolves.not.toThrow();
    });
  });

  // Slice 17 — enableBot builds POST to correct path, no body
  describe("enableBot request building", () => {
    it("sends a POST request", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.enableBot("alpha");
      expect(captured[0].init?.method).toBe("POST");
    });

    it("sends to /admin/bots/{team}/enable", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.enableBot("alpha");
      expect(captured[0].url).toBe(`${BASE}/admin/bots/alpha/enable`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.enableBot("alpha");
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.enableBot("alpha");
      expect(captured[0].init?.body).toBeUndefined();
    });
  });

  // Slice 18 — enableBot response mapping
  describe("enableBot response mapping", () => {
    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.enableBot("alpha");
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.enableBot("alpha");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.enableBot("alpha");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.enableBot("alpha")).resolves.not.toThrow();
    });
  });

  // Slice 19 — setDefaultBot builds POST to /admin/default-bot with binary body
  describe("setDefaultBot request building", () => {
    it("sends a POST request", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setDefaultBot(SAMPLE_WASM);
      expect(captured[0].init?.method).toBe("POST");
    });

    it("sends to /admin/default-bot", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setDefaultBot(SAMPLE_WASM);
      expect(captured[0].url).toBe(`${BASE}/admin/default-bot`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setDefaultBot(SAMPLE_WASM);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("passes the binary body through to fetch", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setDefaultBot(SAMPLE_WASM);
      expect(captured[0].init?.body).toBe(SAMPLE_WASM);
    });

    it("sets Content-Type: application/octet-stream", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setDefaultBot(SAMPLE_WASM);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Content-Type"]).toBe("application/octet-stream");
    });
  });

  // Slice 20 — setDefaultBot response mapping (200/400/401/other)
  describe("setDefaultBot response mapping", () => {
    it("returns { ok: true, badRequest: false, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.setDefaultBot(SAMPLE_WASM);
      expect(result).toEqual({ ok: true, badRequest: false, unauthorized: false });
    });

    it("returns { ok: false, badRequest: true, unauthorized: false } on 400", async () => {
      const { fetchFn } = makeFakeFetch(400, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.setDefaultBot(SAMPLE_WASM);
      expect(result).toEqual({ ok: false, badRequest: true, unauthorized: false });
    });

    it("returns { ok: false, badRequest: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.setDefaultBot(SAMPLE_WASM);
      expect(result).toEqual({ ok: false, badRequest: false, unauthorized: true });
    });

    it("returns { ok: false, badRequest: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.setDefaultBot(SAMPLE_WASM);
      expect(result).toEqual({ ok: false, badRequest: false, unauthorized: false });
    });

    it("does not throw on 400 (bad WASM)", async () => {
      const { fetchFn } = makeFakeFetch(400, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.setDefaultBot(SAMPLE_WASM)).resolves.not.toThrow();
    });
  });

  // Slice 21 — clearDefaultBot builds DELETE to /admin/default-bot, no body
  describe("clearDefaultBot request building", () => {
    it("sends a DELETE request", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.clearDefaultBot();
      expect(captured[0].init?.method).toBe("DELETE");
    });

    it("sends to /admin/default-bot", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.clearDefaultBot();
      expect(captured[0].url).toBe(`${BASE}/admin/default-bot`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.clearDefaultBot();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.clearDefaultBot();
      expect(captured[0].init?.body).toBeUndefined();
    });
  });

  // Slice 22 — clearDefaultBot response mapping
  describe("clearDefaultBot response mapping", () => {
    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.clearDefaultBot();
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.clearDefaultBot();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.clearDefaultBot();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.clearDefaultBot()).resolves.not.toThrow();
    });
  });

  // Slice 23 — uploadTeamBot URL-encodes team names with special chars
  describe("uploadTeamBot URL encoding", () => {
    it("URL-encodes a team name containing a space", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.uploadTeamBot("team a", SAMPLE_WASM);
      expect(captured[0].url).toBe(`${BASE}/admin/bots/team%20a`);
    });

    it("URL-encodes a team name containing a slash", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.uploadTeamBot("team/x", SAMPLE_WASM);
      expect(captured[0].url).toBe(`${BASE}/admin/bots/team%2Fx`);
    });

    it("URL-encodes disableBot team names with special chars", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.disableBot("team a");
      expect(captured[0].url).toBe(`${BASE}/admin/bots/team%20a/disable`);
    });

    it("URL-encodes enableBot team names with special chars", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.enableBot("team/x");
      expect(captured[0].url).toBe(`${BASE}/admin/bots/team%2Fx/enable`);
    });
  });

  // ── Issue 11: startMatch slices ───────────────────────────────────────────

  // Slice 24 — startMatch builds POST with JSON body + auth header
  describe("startMatch request building", () => {
    it("sends a POST request to /admin/matches", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live" });
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/matches`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live" });
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sets Content-Type: application/json", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "headless" });
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Content-Type"]).toBe("application/json");
    });

    it("includes mode in the JSON body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "headless" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "headless" });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(body.mode).toBe("headless");
    });

    it("includes optional seed when provided", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live", seed: 42 });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(body.seed).toBe(42);
    });

    it("omits seed when not provided", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live" });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(Object.prototype.hasOwnProperty.call(body, "seed")).toBe(false);
    });

    it("includes optional maxTicks when provided", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live", maxTicks: 1000 });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(body.maxTicks).toBe(1000);
    });

    it("omits maxTicks when not provided", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live" });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(Object.prototype.hasOwnProperty.call(body, "maxTicks")).toBe(false);
    });

    it("includes optional teams when provided", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live", teams: ["alpha", "beta"] });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(body.teams).toEqual(["alpha", "beta"]);
    });

    it("omits teams when not provided", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live" });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(Object.prototype.hasOwnProperty.call(body, "teams")).toBe(false);
    });

    it("includes optional tps when provided", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live", tps: 60 });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(body.tps).toBe(60);
    });

    it("omits tps when not provided", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { matchId: "m1", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startMatch({ mode: "live" });
      const body = JSON.parse(captured[0].init?.body as string);
      expect(Object.prototype.hasOwnProperty.call(body, "tps")).toBe(false);
    });
  });

  // Slice 25 — startMatch response mapping
  describe("startMatch response mapping", () => {
    it("returns { ok: true, matchId, mode, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, { matchId: "abc-123", mode: "live" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startMatch({ mode: "live" });
      expect(result.ok).toBe(true);
      expect(result.matchId).toBe("abc-123");
      expect(result.mode).toBe("live");
      expect(result.unauthorized).toBe(false);
    });

    it("returns headless mode from server response", async () => {
      const { fetchFn } = makeFakeFetch(200, { matchId: "xyz-456", mode: "headless" });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startMatch({ mode: "headless" });
      expect(result.mode).toBe("headless");
      expect(result.matchId).toBe("xyz-456");
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startMatch({ mode: "live" });
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
      expect(result.matchId).toBe("");
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startMatch({ mode: "live" });
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.startMatch({ mode: "live" })).resolves.not.toThrow();
    });

    it("does not throw on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.startMatch({ mode: "live" })).resolves.not.toThrow();
    });
  });

  // Slice 26 — pauseMatch / resumeMatch request building + response mapping
  describe("pauseMatch request building", () => {
    it("sends POST to /admin/matches/{id}/pause", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.pauseMatch("m1");
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/matches/m1/pause`);
    });

    it("attaches auth header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.pauseMatch("m1");
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.pauseMatch("m1");
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.pauseMatch("m1");
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.pauseMatch("m1");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.pauseMatch("m1");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.pauseMatch("m1")).resolves.not.toThrow();
    });
  });

  describe("resumeMatch request building", () => {
    it("sends POST to /admin/matches/{id}/resume", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.resumeMatch("m1");
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/matches/m1/resume`);
    });

    it("attaches auth header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.resumeMatch("m1");
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.resumeMatch("m1");
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.resumeMatch("m1");
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.resumeMatch("m1");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.resumeMatch("m1")).resolves.not.toThrow();
    });
  });

  // Slice 27 — abortMatch issues DELETE
  describe("abortMatch request building", () => {
    it("sends DELETE to /admin/matches/{id}", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.abortMatch("m1");
      expect(captured[0].init?.method).toBe("DELETE");
      expect(captured[0].url).toBe(`${BASE}/admin/matches/m1`);
    });

    it("attaches auth header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.abortMatch("m1");
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.abortMatch("m1");
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.abortMatch("m1");
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.abortMatch("m1");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.abortMatch("m1");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.abortMatch("m1")).resolves.not.toThrow();
    });
  });

  // Slice 28 — setMatchTps posts JSON {tps}
  describe("setMatchTps request building", () => {
    it("sends POST to /admin/matches/{id}/tps", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setMatchTps("m1", 60);
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/matches/m1/tps`);
    });

    it("attaches auth header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setMatchTps("m1", 60);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sets Content-Type: application/json", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setMatchTps("m1", 60);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Content-Type"]).toBe("application/json");
    });

    it("sends JSON body { tps: <value> }", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setMatchTps("m1", 60);
      const body = JSON.parse(captured[0].init?.body as string);
      expect(body).toEqual({ tps: 60 });
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.setMatchTps("m1", 30);
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.setMatchTps("m1", 30);
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.setMatchTps("m1", 30)).resolves.not.toThrow();
    });
  });

  // Slice 29 — getExhibition parses { running, matchCount }
  describe("getExhibition request building and response mapping", () => {
    it("sends GET to /admin/exhibition", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { running: true, matchCount: 5 });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getExhibition();
      expect(captured[0].init?.method).toBe("GET");
      expect(captured[0].url).toBe(`${BASE}/admin/exhibition`);
    });

    it("attaches auth header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { running: false, matchCount: 0 });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getExhibition();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("returns { ok: true, running: true, matchCount: 5, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, { running: true, matchCount: 5 });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.getExhibition();
      expect(result.ok).toBe(true);
      expect(result.running).toBe(true);
      expect(result.matchCount).toBe(5);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: true, running: false, matchCount: 0, unauthorized: false } when stopped", async () => {
      const { fetchFn } = makeFakeFetch(200, { running: false, matchCount: 0 });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.getExhibition();
      expect(result.ok).toBe(true);
      expect(result.running).toBe(false);
      expect(result.matchCount).toBe(0);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.getExhibition();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.getExhibition();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.getExhibition()).resolves.not.toThrow();
    });
  });

  // Slice 30 — startExhibition / stopExhibition
  describe("startExhibition request building and response mapping", () => {
    it("sends POST to /admin/exhibition/start", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startExhibition();
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/exhibition/start`);
    });

    it("attaches auth header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startExhibition();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startExhibition();
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startExhibition();
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startExhibition();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.startExhibition()).resolves.not.toThrow();
    });
  });

  describe("stopExhibition request building and response mapping", () => {
    it("sends POST to /admin/exhibition/stop", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.stopExhibition();
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/exhibition/stop`);
    });

    it("attaches auth header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.stopExhibition();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.stopExhibition();
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.stopExhibition();
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.stopExhibition();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.stopExhibition()).resolves.not.toThrow();
    });
  });

  // Slice 31 — match id URL-encoding
  describe("match id URL-encoding", () => {
    it("URL-encodes pauseMatch id containing a slash", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.pauseMatch("match/1");
      expect(captured[0].url).toBe(`${BASE}/admin/matches/match%2F1/pause`);
    });

    it("URL-encodes resumeMatch id containing a space", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.resumeMatch("match 1");
      expect(captured[0].url).toBe(`${BASE}/admin/matches/match%201/resume`);
    });

    it("URL-encodes abortMatch id containing special chars", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.abortMatch("match/1");
      expect(captured[0].url).toBe(`${BASE}/admin/matches/match%2F1`);
    });

    it("URL-encodes setMatchTps id containing special chars", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.setMatchTps("match/1", 30);
      expect(captured[0].url).toBe(`${BASE}/admin/matches/match%2F1/tps`);
    });
  });

  // ── Issue 12: Ladder & Replay slices ─────────────────────────────────────

  const SAMPLE_STANDINGS: LadderStanding[] = [
    { competitor: "alpha", mu: 30, sigma: 2.5, conservativeSkill: 22.5, matches: 10 },
    { competitor: "beta",  mu: 25, sigma: 3.0, conservativeSkill: 16.0, matches: 5  },
  ];

  const SAMPLE_RECORDINGS: RecordingListItem[] = [
    { matchId: "rec-1", seed: 42,  tickCount: 1000, winner: "alpha", scores: { alpha: 3, beta: 1 } },
    { matchId: "rec-2", seed: 99,  tickCount: 800,  winner: null,    scores: { alpha: 2, beta: 2 } },
  ];

  // Slice 32 — getLadderStandings GETs /ladder/standings and parses typed array
  describe("getLadderStandings request building and response parsing", () => {
    it("sends a GET request to /ladder/standings", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, SAMPLE_STANDINGS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getLadderStandings();
      expect(captured[0].init?.method).toBe("GET");
      expect(captured[0].url).toBe(`${BASE}/ladder/standings`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, SAMPLE_STANDINGS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getLadderStandings();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("returns a typed LadderStanding[] on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, SAMPLE_STANDINGS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const standings = await client.getLadderStandings();
      expect(standings).toHaveLength(2);
      expect(standings[0].competitor).toBe("alpha");
      expect(standings[0].mu).toBe(30);
      expect(standings[0].sigma).toBe(2.5);
      expect(standings[0].conservativeSkill).toBe(22.5);
      expect(standings[0].matches).toBe(10);
    });

    it("returns an empty array when server returns []", async () => {
      const { fetchFn } = makeFakeFetch(200, []);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const standings = await client.getLadderStandings();
      expect(standings).toEqual([]);
    });

    it("throws on non-200 response", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.getLadderStandings()).rejects.toThrow("500");
    });

    it("throws on 401 response", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.getLadderStandings()).rejects.toThrow();
    });
  });

  // Slice 33 — resetLadder POSTs /ladder/reset with auth, maps 200/401/other
  describe("resetLadder request building and response mapping", () => {
    it("sends a POST request to /ladder/reset", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.resetLadder();
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/ladder/reset`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.resetLadder();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.resetLadder();
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.resetLadder();
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.resetLadder();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.resetLadder();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.resetLadder()).resolves.not.toThrow();
    });
  });

  // Slice 34 — getLadderRunner GETs /admin/ladder/runner, parses { running }, maps 401
  describe("getLadderRunner request building and response mapping", () => {
    it("sends a GET request to /admin/ladder/runner", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { running: true });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getLadderRunner();
      expect(captured[0].init?.method).toBe("GET");
      expect(captured[0].url).toBe(`${BASE}/admin/ladder/runner`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, { running: false });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.getLadderRunner();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("returns { ok: true, running: true, unauthorized: false } when runner is active", async () => {
      const { fetchFn } = makeFakeFetch(200, { running: true });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.getLadderRunner();
      expect(result.ok).toBe(true);
      expect(result.running).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: true, running: false, unauthorized: false } when runner is stopped", async () => {
      const { fetchFn } = makeFakeFetch(200, { running: false });
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.getLadderRunner();
      expect(result.ok).toBe(true);
      expect(result.running).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, running: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.getLadderRunner();
      expect(result.ok).toBe(false);
      expect(result.running).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, running: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.getLadderRunner();
      expect(result.ok).toBe(false);
      expect(result.running).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.getLadderRunner()).resolves.not.toThrow();
    });
  });

  // Slice 35 — startLadderRunner POSTs /admin/ladder/runner/start with auth
  describe("startLadderRunner request building and response mapping", () => {
    it("sends a POST request to /admin/ladder/runner/start", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startLadderRunner();
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/ladder/runner/start`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startLadderRunner();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.startLadderRunner();
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startLadderRunner();
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startLadderRunner();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.startLadderRunner();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.startLadderRunner()).resolves.not.toThrow();
    });
  });

  // Slice 36 — stopLadderRunner POSTs /admin/ladder/runner/stop with auth
  describe("stopLadderRunner request building and response mapping", () => {
    it("sends a POST request to /admin/ladder/runner/stop", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.stopLadderRunner();
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/ladder/runner/stop`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.stopLadderRunner();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.stopLadderRunner();
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.stopLadderRunner();
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.stopLadderRunner();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.stopLadderRunner();
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.stopLadderRunner()).resolves.not.toThrow();
    });
  });

  // Slice 37 — listRecordings GETs /recordings and parses camelCase list
  describe("listRecordings request building and response parsing", () => {
    it("sends a GET request to /recordings", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, SAMPLE_RECORDINGS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.listRecordings();
      expect(captured[0].init?.method).toBe("GET");
      expect(captured[0].url).toBe(`${BASE}/recordings`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, SAMPLE_RECORDINGS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.listRecordings();
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("returns a typed RecordingListItem[] on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, SAMPLE_RECORDINGS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const recordings = await client.listRecordings();
      expect(recordings).toHaveLength(2);
      expect(recordings[0].matchId).toBe("rec-1");
      expect(recordings[0].seed).toBe(42);
      expect(recordings[0].tickCount).toBe(1000);
      expect(recordings[0].winner).toBe("alpha");
      expect(recordings[0].scores).toEqual({ alpha: 3, beta: 1 });
    });

    it("preserves null winner for drawn matches", async () => {
      const { fetchFn } = makeFakeFetch(200, SAMPLE_RECORDINGS);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const recordings = await client.listRecordings();
      expect(recordings[1].winner).toBeNull();
    });

    it("returns an empty array when server returns []", async () => {
      const { fetchFn } = makeFakeFetch(200, []);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const recordings = await client.listRecordings();
      expect(recordings).toEqual([]);
    });

    it("throws on non-200 response", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.listRecordings()).rejects.toThrow("500");
    });

    it("throws on 404 response", async () => {
      const { fetchFn } = makeFakeFetch(404, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.listRecordings()).rejects.toThrow();
    });
  });

  // Slice 38 — replayRecording POSTs /recordings/{id}/replay (URL-encoded)
  describe("replayRecording request building and response mapping", () => {
    it("sends a POST request to /recordings/{id}/replay", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.replayRecording("rec-1");
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/recordings/rec-1/replay`);
    });

    it("URL-encodes recording id containing a slash", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.replayRecording("rec/1");
      expect(captured[0].url).toBe(`${BASE}/recordings/rec%2F1/replay`);
    });

    it("URL-encodes recording id containing a space", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.replayRecording("rec 1");
      expect(captured[0].url).toBe(`${BASE}/recordings/rec%201/replay`);
    });

    it("sends no body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.replayRecording("rec-1");
      expect(captured[0].init?.body).toBeUndefined();
    });

    it("returns { ok: true, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.replayRecording("rec-1");
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.replayRecording("rec-1");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on any HTTP status", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.replayRecording("rec-1")).resolves.not.toThrow();
    });
  });

  // Slice 39 — downloadRecording GETs /admin/recordings/{id}/download with auth
  describe("downloadRecording request building and response mapping", () => {
    const SAMPLE_ARTIFACT: RecordingArtifact = {
      match_id: "rec-1",
      seed: 42,
      params: { max_ticks: 300 },
      specs: [{ id: "ship-0", class: "Skiff" }],
      intent_log: [[["ship-0", { thrust: 1.0 }]]],
      meta: {
        match_id: "rec-1",
        seed: 42,
        tick_count: 1000,
        winner: "alpha",
        scores: [["alpha", 3], ["beta", 1]],
      },
    };

    it("sends a GET request to /admin/recordings/{id}/download", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, SAMPLE_ARTIFACT);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.downloadRecording("rec-1");
      expect(captured[0].init?.method).toBe("GET");
      expect(captured[0].url).toBe(`${BASE}/admin/recordings/rec-1/download`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, SAMPLE_ARTIFACT);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.downloadRecording("rec-1");
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("URL-encodes recording id containing a slash", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, SAMPLE_ARTIFACT);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.downloadRecording("rec/1");
      expect(captured[0].url).toBe(`${BASE}/admin/recordings/rec%2F1/download`);
    });

    it("returns { ok: true, data: RecordingArtifact, ... } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, SAMPLE_ARTIFACT);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.downloadRecording("rec-1");
      expect(result.ok).toBe(true);
      expect(result.unauthorized).toBe(false);
      expect(result.notFound).toBe(false);
      expect(result.data).not.toBeNull();
      expect(result.data?.match_id).toBe("rec-1");
      expect(result.data?.seed).toBe(42);
      expect(result.data?.meta.tick_count).toBe(1000);
      expect(result.data?.meta.winner).toBe("alpha");
      expect(result.data?.meta.scores).toEqual([["alpha", 3], ["beta", 1]]);
    });

    it("preserves null winner in downloaded artifact", async () => {
      const artifactWithNull: RecordingArtifact = {
        ...SAMPLE_ARTIFACT,
        meta: { ...SAMPLE_ARTIFACT.meta, winner: null },
      };
      const { fetchFn } = makeFakeFetch(200, artifactWithNull);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.downloadRecording("rec-2");
      expect(result.ok).toBe(true);
      expect(result.data?.meta.winner).toBeNull();
    });

    it("returns { ok: false, unauthorized: true, notFound: false } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.downloadRecording("rec-1");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(true);
      expect(result.notFound).toBe(false);
      expect(result.data).toBeNull();
    });

    it("returns { ok: false, notFound: true, unauthorized: false } on 404", async () => {
      const { fetchFn } = makeFakeFetch(404, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.downloadRecording("no-such-rec");
      expect(result.ok).toBe(false);
      expect(result.notFound).toBe(true);
      expect(result.unauthorized).toBe(false);
      expect(result.data).toBeNull();
    });

    it("returns { ok: false, unauthorized: false, notFound: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.downloadRecording("rec-1");
      expect(result.ok).toBe(false);
      expect(result.unauthorized).toBe(false);
      expect(result.notFound).toBe(false);
      expect(result.data).toBeNull();
    });

    it("does not throw on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.downloadRecording("rec-1")).resolves.not.toThrow();
    });

    it("does not throw on 404", async () => {
      const { fetchFn } = makeFakeFetch(404, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.downloadRecording("rec-1")).resolves.not.toThrow();
    });

    it("does not throw on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.downloadRecording("rec-1")).resolves.not.toThrow();
    });
  });

  // Slice 40 — importRecording POSTs /admin/recordings/import with auth + artifact body
  describe("importRecording request building and response mapping", () => {
    const SAMPLE_ARTIFACT: RecordingArtifact = {
      match_id: "rec-import-1",
      seed: 7,
      params: { max_ticks: 500 },
      specs: [{ id: "ship-0", class: "Skiff" }, { id: "ship-1", class: "Skiff" }],
      intent_log: [[["ship-0", { thrust: 0.5 }]]],
      meta: {
        match_id: "rec-import-1",
        seed: 7,
        tick_count: 500,
        winner: "ship-0",
        scores: [["ship-0", 2], ["ship-1", 0]],
      },
    };

    it("sends a POST request to /admin/recordings/import", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.importRecording(SAMPLE_ARTIFACT);
      expect(captured[0].init?.method).toBe("POST");
      expect(captured[0].url).toBe(`${BASE}/admin/recordings/import`);
    });

    it("attaches Authorization: Facilitator <password> header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.importRecording(SAMPLE_ARTIFACT);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Authorization"]).toBe(`Facilitator ${PASSWORD}`);
    });

    it("attaches Content-Type: application/json header", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.importRecording(SAMPLE_ARTIFACT);
      const headers = captured[0].init?.headers as Record<string, string>;
      expect(headers["Content-Type"]).toBe("application/json");
    });

    it("sends the artifact as JSON body", async () => {
      const { fetchFn, captured } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await client.importRecording(SAMPLE_ARTIFACT);
      const body = captured[0].init?.body as string;
      expect(JSON.parse(body)).toEqual(SAMPLE_ARTIFACT);
    });

    it("returns { ok: true, badRequest: false, unauthorized: false } on 200", async () => {
      const { fetchFn } = makeFakeFetch(200, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.importRecording(SAMPLE_ARTIFACT);
      expect(result.ok).toBe(true);
      expect(result.badRequest).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, badRequest: true, unauthorized: false } on 400", async () => {
      const { fetchFn } = makeFakeFetch(400, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.importRecording(SAMPLE_ARTIFACT);
      expect(result.ok).toBe(false);
      expect(result.badRequest).toBe(true);
      expect(result.unauthorized).toBe(false);
    });

    it("returns { ok: false, badRequest: false, unauthorized: true } on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.importRecording(SAMPLE_ARTIFACT);
      expect(result.ok).toBe(false);
      expect(result.badRequest).toBe(false);
      expect(result.unauthorized).toBe(true);
    });

    it("returns { ok: false, badRequest: false, unauthorized: false } on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      const result = await client.importRecording(SAMPLE_ARTIFACT);
      expect(result.ok).toBe(false);
      expect(result.badRequest).toBe(false);
      expect(result.unauthorized).toBe(false);
    });

    it("does not throw on 400", async () => {
      const { fetchFn } = makeFakeFetch(400, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.importRecording(SAMPLE_ARTIFACT)).resolves.not.toThrow();
    });

    it("does not throw on 401", async () => {
      const { fetchFn } = makeFakeFetch(401, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.importRecording(SAMPLE_ARTIFACT)).resolves.not.toThrow();
    });

    it("does not throw on 500", async () => {
      const { fetchFn } = makeFakeFetch(500, null);
      const client = createAdminClient(BASE, PASSWORD, fetchFn);
      await expect(client.importRecording(SAMPLE_ARTIFACT)).resolves.not.toThrow();
    });
  });
});
