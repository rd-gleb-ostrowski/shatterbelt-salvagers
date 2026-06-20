/**
 * Unit tests for adminFormatters — pure formatting helpers.
 *
 * Covers:
 *   Slice 1  — formatLastSeen: null → "—"
 *   Slice 2  — formatLastSeen: just now (0 ms elapsed) → "0s ago"
 *   Slice 3  — formatLastSeen: 5 seconds elapsed → "5s ago"
 *   Slice 4  — formatLastSeen: 59 s elapsed → "59s ago" (boundary)
 *   Slice 5  — formatLastSeen: 60 s elapsed → "1m ago" (boundary)
 *   Slice 6  — formatLastSeen: 90 s elapsed → "1m ago"
 *   Slice 7  — formatLastSeen: 3599 s elapsed → "59m ago" (boundary)
 *   Slice 8  — formatLastSeen: 3600 s elapsed → "1h ago" (boundary)
 *   Slice 9  — formatLastSeen: 7200 s elapsed → "2h ago"
 *   Slice 10 — formatConnected: true → "● connected"
 *   Slice 11 — formatConnected: false → "○ offline"
 *   Slice 12 — formatKind: "ws" → "WS Bot"
 *   Slice 13 — formatKind: "wasm" → "WASM Bot"
 *   Slice 14 — formatKind: "default" → "Default Bot"
 *   Slice 15 — formatKind: unknown kind → returned verbatim
 */

import { describe, it, expect } from "vitest";
import {
  formatLastSeen,
  formatConnected,
  formatKind,
} from "./adminFormatters.ts";

// ── formatLastSeen ────────────────────────────────────────────────────────────

describe("formatLastSeen", () => {
  const NOW = 1_700_000_000_000; // fixed reference point

  it("returns '—' for null (never seen)", () => {
    expect(formatLastSeen(null, NOW)).toBe("—");
  });

  it("returns '0s ago' when elapsed is 0 ms", () => {
    expect(formatLastSeen(NOW, NOW)).toBe("0s ago");
  });

  it("returns '5s ago' when 5 000 ms elapsed", () => {
    expect(formatLastSeen(NOW - 5_000, NOW)).toBe("5s ago");
  });

  it("returns '59s ago' at the 59-second boundary", () => {
    expect(formatLastSeen(NOW - 59_000, NOW)).toBe("59s ago");
  });

  it("returns '1m ago' at exactly 60 seconds elapsed", () => {
    expect(formatLastSeen(NOW - 60_000, NOW)).toBe("1m ago");
  });

  it("returns '1m ago' for 90 seconds elapsed", () => {
    expect(formatLastSeen(NOW - 90_000, NOW)).toBe("1m ago");
  });

  it("returns '59m ago' at 3 599 s elapsed (boundary before hours)", () => {
    expect(formatLastSeen(NOW - 3_599_000, NOW)).toBe("59m ago");
  });

  it("returns '1h ago' at exactly 3 600 s elapsed", () => {
    expect(formatLastSeen(NOW - 3_600_000, NOW)).toBe("1h ago");
  });

  it("returns '2h ago' for 7 200 s elapsed", () => {
    expect(formatLastSeen(NOW - 7_200_000, NOW)).toBe("2h ago");
  });
});

// ── formatConnected ───────────────────────────────────────────────────────────

describe("formatConnected", () => {
  it("returns '● connected' for true", () => {
    expect(formatConnected(true)).toBe("● connected");
  });

  it("returns '○ offline' for false", () => {
    expect(formatConnected(false)).toBe("○ offline");
  });
});

// ── formatKind ────────────────────────────────────────────────────────────────

describe("formatKind", () => {
  it("formats 'ws' as 'WS Bot'", () => {
    expect(formatKind("ws")).toBe("WS Bot");
  });

  it("formats 'wasm' as 'WASM Bot'", () => {
    expect(formatKind("wasm")).toBe("WASM Bot");
  });

  it("formats 'default' as 'Default Bot'", () => {
    expect(formatKind("default")).toBe("Default Bot");
  });

  it("returns unknown kind verbatim (forward-compatible)", () => {
    expect(formatKind("hybrid")).toBe("hybrid");
  });

  it("returns empty string verbatim", () => {
    expect(formatKind("")).toBe("");
  });
});
