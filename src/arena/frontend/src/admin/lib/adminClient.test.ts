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
});
