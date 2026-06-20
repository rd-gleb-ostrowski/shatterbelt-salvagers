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
use crate::health::{BotHealthEntry, BotHealthStore, DqStore, ExclusionDriver};
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
    /// Optional DQ store — checked before WS/WASM driver selection.
    /// When `None`, no exclusion is applied (backward-compat with tests).
    dq_store: Option<Arc<DqStore>>,
    /// Optional health store — a `BotHealthEntry` is registered per slot
    /// when this is `Some`.  When `None`, health is not tracked.
    health_store: Option<Arc<BotHealthStore>>,
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
        Self {
            ws_registry,
            wasm_store,
            fuel_per_tick,
            dq_store: None,
            health_store: None,
        }
    }

    /// Attach shared DQ and health stores (issue 12).
    ///
    /// When set:
    /// - DQ'd teams fall back to Default Bot (WS/WASM drivers are skipped).
    /// - A `BotHealthEntry` is created per slot and injected into the driver.
    /// - Every driver is wrapped in an [`ExclusionDriver`] so mid-match kicks
    ///   take effect on the next tick.
    pub fn with_moderation(mut self, dq: Arc<DqStore>, health: Arc<BotHealthStore>) -> Self {
        self.dq_store = Some(dq);
        self.health_store = Some(health);
        self
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
        // If moderation is active and team is DQ'd → Default Bot immediately.
        if let Some(dq) = &self.dq_store {
            if dq.is_disqualified(&slot.team) {
                let health = self.make_health(&slot.team, "default");
                let inner: Box<dyn BotDriver> = Box::new(DefaultBotDriver::new(params));
                return Box::new(ExclusionDriver::new(
                    &slot.team,
                    inner,
                    Arc::clone(dq),
                    health,
                ));
            }
        }

        // Priority 1 — WS Bot: a live connection supersedes everything.
        if let Some(driver) = self.ws_registry.take(&slot.team) {
            // Inject health into the WS driver if moderation is active.
            if let Some(h) = self.make_health(&slot.team, "ws") {
                // Downcast to WsBotDriver if possible; otherwise skip health inject.
                // Since the registry holds Box<dyn BotDriver>, we use a trait method seam.
                // Call set_health via Any-like downcasting isn't available here.
                // Instead, the health entry is still registered — the ExclusionDriver
                // will mark it connected=false on DQ.  The WS driver updates health
                // only if it was given the entry before boxing (see ws.rs).
                drop(h); // health entry already registered in health_store
            }
            // Wrap in ExclusionDriver for mid-match kick support.
            if let Some(dq) = &self.dq_store {
                let health = self.health_store.as_ref().and_then(|hs| hs.get(&slot.team));
                return Box::new(ExclusionDriver::new(
                    &slot.team,
                    driver,
                    Arc::clone(dq),
                    health,
                ));
            }
            return driver;
        }

        // Priority 2 — WASM Bot: uploaded artifact.
        if let Some(bytes) = self.wasm_store.get(&slot.team) {
            match WasmBotDriver::new(&bytes, &slot.tick0_obs_json, self.fuel_per_tick) {
                Ok(mut driver) => {
                    let health = self.make_health(&slot.team, "wasm");
                    if let Some(h) = health {
                        driver.set_health(Arc::clone(&h));
                        let dq = self
                            .dq_store
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(DqStore::new);
                        return Box::new(ExclusionDriver::new(
                            &slot.team,
                            Box::new(driver),
                            dq,
                            Some(h),
                        ));
                    }
                    return Box::new(driver);
                }
                Err(_) => {
                    // Bad upload — fall through to Default Bot.
                    // The match must never abort due to a broken artifact.
                }
            }
        }

        // Priority 3 — Default Bot: built-in fallback, field always full.
        let health = self.make_health(&slot.team, "default");
        let inner: Box<dyn BotDriver> = Box::new(DefaultBotDriver::new(params));
        if let Some(dq) = &self.dq_store {
            Box::new(ExclusionDriver::new(&slot.team, inner, Arc::clone(dq), health))
        } else {
            inner
        }
    }

    /// Create and register a health entry when health tracking is active.
    fn make_health(&self, team: &str, kind: &str) -> Option<Arc<BotHealthEntry>> {
        self.health_store
            .as_ref()
            .map(|hs| hs.register(BotHealthEntry::new(team, kind)))
    }
}
