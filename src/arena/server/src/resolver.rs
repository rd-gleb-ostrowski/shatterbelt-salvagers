//! Connection resolver — per-slot WS → WASM → Default Bot priority
//! (ADR-0001, issue 06).
//!
//! [`ConnectionResolver`] is the seam that stitches together all three driver
//! kinds into the `Vec<Box<dyn BotDriver>>` that
//! [`crate::runner::MatchRunner`] consumes.
//!
//! ## Priority (ADR-0001)
//!
//! For each team slot, in order:
//!
//! 1. **WS Bot** — a live [`WsBotDriver`](crate::ws::WsBotDriver) in the
//!    [`WsConnectionRegistry`] for this team.
//! 2. **WASM Bot** — an artifact in the [`WasmBotStore`] for this team.
//! 3. **Default Bot** — the built-in
//!    [`DefaultBotDriver`](crate::bot::DefaultBotDriver) fallback so every
//!    slot always plays.
//!
//! ## Seams for future issues
//!
//! | Future issue | Usage |
//! |---|---|
//! | 09 (headless) | Pass an empty [`WsConnectionRegistry`] — WS drivers are never present in headless-fast matches (ADR story 23) |
//! | 11 (admin start) | Call [`ConnectionResolver::resolve`] to build drivers; hold `Arc<WsConnectionRegistry>` to inject live bots before calling it |
//! | 12 (bot health) | Read health from registered [`WsBotDriver`]s via the registry before the match |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arena_engine::Params;

use crate::bot::DefaultBotDriver;
use crate::runner::BotDriver;
use crate::store::WasmBotStore;
use crate::wasm_host::WasmBotDriver;

// ── WsConnectionRegistry ─────────────────────────────────────────────────────

/// Registry of live WS bot connections, keyed by team identity.
///
/// Each entry is a [`BotDriver`] already wired to a live WebSocket connection
/// (typically a [`WsBotDriver`](crate::ws::WsBotDriver)). The WS handler
/// inserts drivers here after a successful join/assigned handshake; the
/// [`ConnectionResolver`] takes them out at match-build time.
///
/// ## Thread safety
///
/// The registry is `Send + Sync` and should be shared as
/// `Arc<WsConnectionRegistry>`.
///
/// ## Integration with ws.rs (issue 11)
///
/// After the WS handshake is complete (successful `assigned`), the WS handler
/// calls `registry.insert(team, Box::new(ws_driver))`.  Issue 11 (admin starts
/// a match) then calls `resolver.resolve(…)` which takes the driver from the
/// registry, transferring ownership to the new [`MatchRunner`].
#[derive(Default)]
pub struct WsConnectionRegistry {
    connections: Mutex<HashMap<String, Box<dyn BotDriver>>>,
}

impl WsConnectionRegistry {
    /// Create a new, empty registry wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register a live WS driver for `team`.
    ///
    /// Replaces any previously registered driver for the same team (e.g. on
    /// reconnect). Called by the WS handler after a successful join/assigned
    /// handshake so the resolver can find it at match-build time.
    pub fn insert(&self, team: impl Into<String>, driver: Box<dyn BotDriver>) {
        self.connections.lock().unwrap().insert(team.into(), driver);
    }

    /// Take the live driver for `team`, removing it from the registry.
    ///
    /// Called by the resolver at match-build time. Consuming the registration
    /// ensures each match starts with exclusive ownership of every driver.
    pub fn take(&self, team: &str) -> Option<Box<dyn BotDriver>> {
        self.connections.lock().unwrap().remove(team)
    }

    /// Return `true` if a live driver is registered for `team`.
    ///
    /// Inspects without consuming; useful for health checks (issue 12).
    pub fn has(&self, team: &str) -> bool {
        self.connections.lock().unwrap().contains_key(team)
    }
}

// ── Slot ──────────────────────────────────────────────────────────────────────

/// One match slot: team identity + pre-computed tick-0 observation JSON for
/// WASM bot warm-up.
///
/// Build one `Slot` per ship/team before calling
/// [`ConnectionResolver::resolve`].  The `tick0_obs_json` is the serialised
/// PROTOCOL §6 `tick` message for this ship at tick 0, produced by
/// [`crate::ws::obs_to_tick_json`] against a freshly-created
/// [`arena_engine::Engine`] (before any `step` calls).  It is only consumed
/// by WASM slots; WS and Default Bot slots ignore it (an empty string is
/// acceptable for those).
pub struct Slot {
    /// Team identity — must match the key used in [`WsConnectionRegistry`]
    /// and [`WasmBotStore`].
    pub team: String,

    /// Pre-serialised tick-0 observation JSON for WASM warm-up.
    ///
    /// Passed verbatim to [`WasmBotDriver::new`] as `tick0_obs_json`.
    /// Unused for WS and Default Bot slots.
    pub tick0_obs_json: String,
}

// ── ConnectionResolver ────────────────────────────────────────────────────────

/// Builds the per-slot [`BotDriver`] list for one match (ADR-0001 priority).
///
/// Create one resolver and share it (via `Arc`) across the headless runner
/// (issue 09) and the admin match-start handler (issue 11).
///
/// ## Example
///
/// ```no_run
/// use std::sync::Arc;
/// use arena_engine::{Engine, Params, ShipSpec};
/// use arena_server::{
///     resolver::{ConnectionResolver, Slot, WsConnectionRegistry},
///     store::WasmBotStore,
///     ws::obs_to_tick_json,
/// };
///
/// # let wasm_store = WasmBotStore::new();
/// # let ws_registry = WsConnectionRegistry::new();
/// # let params = Params::default();
/// # let specs: Vec<ShipSpec> = vec![];
/// # let teams: Vec<String> = vec![];
/// let resolver = ConnectionResolver::new(
///     Arc::clone(&ws_registry),
///     Arc::clone(&wasm_store),
///     10_000_000,
/// );
///
/// // Build the engine first, then extract tick-0 obs for WASM warm-up.
/// let engine = Engine::new(42, params.clone(), specs.clone());
/// let slots: Vec<Slot> = teams.iter().zip(specs.iter()).map(|(team, spec)| {
///     let obs = engine.observation(&spec.id).unwrap();
///     Slot {
///         team: team.clone(),
///         tick0_obs_json: obs_to_tick_json(0, &obs),
///     }
/// }).collect();
///
/// let drivers = resolver.resolve(&slots, &params);
/// ```
pub struct ConnectionResolver {
    ws_registry: Arc<WsConnectionRegistry>,
    wasm_store: Arc<WasmBotStore>,
    /// Wasmtime fuel budget per `tick` call. `10_000_000` is a generous
    /// default; tests may pass a smaller value to trigger exhaustion.
    fuel_per_tick: u64,
}

impl ConnectionResolver {
    /// Construct a resolver.
    ///
    /// - `ws_registry` — shared live-WS-connection registry (wired into the
    ///   WS handler by issue 11).
    /// - `wasm_store` — shared WASM artifact store (wired into `POST /bots`).
    /// - `fuel_per_tick` — wasmtime instruction budget per
    ///   [`WasmBotDriver`] tick call.
    pub fn new(
        ws_registry: Arc<WsConnectionRegistry>,
        wasm_store: Arc<WasmBotStore>,
        fuel_per_tick: u64,
    ) -> Self {
        Self { ws_registry, wasm_store, fuel_per_tick }
    }

    /// Build the per-slot driver list for one match.
    ///
    /// Returns one [`BotDriver`] per slot in the same order as `slots`.
    /// The field is always full — every slot is assigned exactly one driver.
    ///
    /// If a WASM bot fails to instantiate (e.g. bad upload), the slot
    /// silently falls back to the Default Bot so the match is never aborted.
    ///
    /// WS drivers are consumed from the registry; those teams' entries are
    /// absent from the registry after this call returns.
    pub fn resolve(&self, slots: &[Slot], params: &Params) -> Vec<Box<dyn BotDriver>> {
        slots.iter().map(|slot| self.resolve_slot(slot, params)).collect()
    }

    fn resolve_slot(&self, slot: &Slot, params: &Params) -> Box<dyn BotDriver> {
        // Priority 1 — WS Bot: a live connection supersedes everything.
        if let Some(driver) = self.ws_registry.take(&slot.team) {
            return driver;
        }

        // Priority 2 — WASM Bot: uploaded artifact.
        if let Some(bytes) = self.wasm_store.get(&slot.team) {
            match WasmBotDriver::new(&bytes, &slot.tick0_obs_json, self.fuel_per_tick) {
                Ok(driver) => return Box::new(driver),
                Err(_) => {
                    // Bad upload — fall through to Default Bot.
                    // The match must never abort due to a broken artifact.
                }
            }
        }

        // Priority 3 — Default Bot: built-in fallback, field always full.
        Box::new(DefaultBotDriver::new(params))
    }
}
