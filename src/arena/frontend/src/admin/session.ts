/**
 * session — in-memory auth/session holder for the Admin console.
 *
 * Holds the facilitator password and a ready-to-use AdminClient for the
 * duration of the browser session (cleared on sign-out or page reload).
 *
 * Seam: issues 09-12 import `getSession()` to obtain the client and call
 * admin-gated endpoints without re-entering the password.
 *
 *   import { getSession } from "../session.ts";
 *   const session = getSession();
 *   if (!session) { /* redirect to sign-in * / return; }
 *   await session.client.getBots(); // or whichever endpoint
 */

import type { AdminClient } from "./lib/adminClient.ts";

// ── Types ─────────────────────────────────────────────────────────────────────

/** The active admin session. Null when not signed in. */
export interface AdminSession {
  /** The facilitator password (held in-memory; never persisted). */
  readonly password: string;
  /**
   * Typed API client pre-configured with the password.
   *
   * Issues 09-12 call methods on this client directly without needing to know
   * the password or construct headers.
   */
  readonly client: AdminClient;
}

// ── Module-level state ────────────────────────────────────────────────────────

let _session: AdminSession | null = null;
const ADMIN_PW_KEY = "shatterbeltSalvagersAdminPw"

// ── API ───────────────────────────────────────────────────────────────────────

/**
 * Store a new session after successful sign-in.
 *
 * @param password The facilitator password (verified by verifyAuth before calling this).
 * @param client   The AdminClient configured with that password.
 * @returns        The stored session (convenience for chaining).
 */
export function setSession(password: string, client: AdminClient): AdminSession {
  _session = { password, client };
  localStorage.setItem(ADMIN_PW_KEY, password)
  return _session;
}

/**
 * Retrieve the current session, or `null` if not signed in.
 *
 * Issues 09-12 call this at the start of each action; if `null`, they should
 * redirect the user back to the sign-in view.
 */
export function getSession(): AdminSession | null {
  return _session;
}

/**
 * Clear the session on sign-out.
 *
 * Call before rendering the sign-in view again.
 */
export function clearSession(): void {
  _session = null;
  localStorage.removeItem(ADMIN_PW_KEY)
}

export function getStoredPassword(): string | null {
  return localStorage.getItem(ADMIN_PW_KEY)
}
