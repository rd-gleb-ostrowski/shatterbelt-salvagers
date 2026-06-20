/**
 * adminFormatters — pure, framework-free formatting helpers for the bot health dashboard.
 *
 * Exports:
 *   formatLastSeen(ms, nowMs?)  — unix-ms timestamp (or null) → human age string
 *   formatConnected(connected)  — boolean → readable status label
 *   formatKind(kind)            — driver kind string → display label
 *
 * All functions are pure and deterministic given the same inputs.
 * `nowMs` defaults to `Date.now()` so callers don't need to pass it; tests
 * pass a fixed value to achieve determinism.
 */

// ── Last-seen age ─────────────────────────────────────────────────────────────

/**
 * Format a unix-millisecond timestamp (or null) as a human-readable age.
 *
 * Rules:
 *   null                 → "—"           (bot never seen)
 *   age < 60 s           → "Xs ago"      (e.g. "5s ago")
 *   60 s ≤ age < 3600 s  → "Xm ago"     (e.g. "2m ago")
 *   age ≥ 3600 s         → "Xh ago"     (e.g. "1h ago")
 *
 * @param ms     Unix timestamp in milliseconds, or `null` for "never seen".
 * @param nowMs  Reference "now" in milliseconds. Defaults to `Date.now()`.
 *               Pass a fixed value in tests for determinism.
 */
export function formatLastSeen(ms: number | null, nowMs: number = Date.now()): string {
  if (ms === null) return "—";
  const ageMs = Math.max(0, nowMs - ms);
  const ageSec = Math.floor(ageMs / 1000);
  if (ageSec < 60) return `${ageSec}s ago`;
  const ageMin = Math.floor(ageSec / 60);
  if (ageMin < 60) return `${ageMin}m ago`;
  const ageHr = Math.floor(ageMin / 60);
  return `${ageHr}h ago`;
}

// ── Connected status ──────────────────────────────────────────────────────────

/**
 * Format a bot's connected boolean as a readable status label.
 *
 *   true  → "● connected"
 *   false → "○ offline"
 */
export function formatConnected(connected: boolean): string {
  return connected ? "● connected" : "○ offline";
}

// ── Driver kind ───────────────────────────────────────────────────────────────

/**
 * Expand a driver kind code into a human-readable label.
 *
 *   "ws"      → "WS Bot"
 *   "wasm"    → "WASM Bot"
 *   "default" → "Default Bot"
 *   other     → returned verbatim (forward-compatible with future kinds)
 */
export function formatKind(kind: string): string {
  switch (kind) {
    case "ws":
      return "WS Bot";
    case "wasm":
      return "WASM Bot";
    case "default":
      return "Default Bot";
    default:
      return kind;
  }
}
