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
//! 1. **WS Bot** — a live [`WsBotDriver`](crate::ws::WsBotDriver) minted from
//!    a [`BotSessionSource`] in the [`WsConnectionRegistry`] for this team.
//! 2. **WASM Bot** — an artifact in the [`WasmBotStore`] for this team.
//! 3. **Default Bot** — the built-in
//!    [`DefaultBotDriver`](crate::bot::DefaultBotDriver) fallback so every
//!    slot always plays.
//!
//! ## Persistent connection model (ADR-0001 v2)
//!
//! A WS bot connects once and stays connected across successive matches.
//! The registry stores an `Arc<dyn BotSessionSource>` per team; each
//! match calls `BotSessionSource::make_driver` to mint a fresh
//! per-match driver without consuming the session.
//!
//! ## Seams for future issues
//!
//! | Future issue | Usage |
//! |---|---|
//! | 09 (headless) | Pass an empty [`WsConnectionRegistry`] — WS drivers are never present in headless-fast matches (ADR story 23) |
//! | 11 (admin start) | Call [`ConnectionResolver::resolve`] to build drivers; `ws_registry.make_driver` mints a fresh driver each time |
//! | 12 (bot health) | Read health from registered sessions via the registry before the match |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_engine::Params;

use crate::bot::DefaultBotDriver;
use crate::health::{BotHealthEntry, BotHealthStore, DqStore, ExclusionDriver};
use crate::runner::BotDriver;
use crate::store::{DefaultBotStore, DisabledStore, WasmBotStore};
use crate::wasm_host::WasmBotDriver;

// ── BotSessionSource ──────────────────────────────────────────────────────────

/// Trait for a persistent WS bot session that mints per-match drivers.
///
/// Implemented by [`crate::ws::WsSession`] (production) and stub types
/// (tests).  The registry stores `Arc<dyn BotSessionSource>` so the resolver
/// can mint a fresh [`BotDriver`] for each match without consuming the
/// session.
///
/// ## Thread safety
///
/// Implementations must be `Send + Sync`; they are shared via `Arc`.
pub trait BotSessionSource: Send + Sync {
    /// Mint a fresh per-match [`BotDriver`] backed by this session.
    ///
    /// Called at match-build time once per match.  The same underlying
    /// WebSocket connection is reused across calls.
    fn make_driver(
        &self,
        deadline: Duration,
        health: Option<Arc<BotHealthEntry>>,
    ) -> Box<dyn BotDriver>;

    /// Queue a raw JSON envelope frame to the connected bot (fire-and-forget).
    ///
    /// Returns `true` if the frame was queued, `false` if the channel is full
    /// or the socket has already closed.  Callers may log a warning on `false`
    /// but must not treat it as fatal.
    fn try_send_envelope(&self, json: String) -> bool;
}

// ── WsConnectionRegistry ─────────────────────────────────────────────────────

/// Registry of live WS bot sessions, keyed by team identity.
///
/// Each entry is an `Arc<dyn BotSessionSource>` for a bot whose WebSocket
/// connection is currently open.  The WS handler registers a session here
/// after a successful join/assigned handshake; the [`ConnectionResolver`]
/// mints a fresh [`BotDriver`] from the session at each match-build time
/// without removing it from the registry.
///
/// ## Thread safety
///
/// The registry is `Send + Sync` and should be shared as
/// `Arc<WsConnectionRegistry>`.
///
/// ## Persistent connection model
///
/// Unlike the old consume-once `take` API, sessions survive match boundaries.
/// The WS handler calls `remove` when the bot disconnects.
#[derive(Default)]
pub struct WsConnectionRegistry {
    connections: Mutex<HashMap<String, Arc<dyn BotSessionSource>>>,
}

impl WsConnectionRegistry {
    /// Create a new, empty registry wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register a persistent session for `team`.
    ///
    /// Replaces any previously registered session for the same team (e.g. on
    /// reconnect).  Called by the WS handler after a successful join/assigned
    /// handshake.
    pub fn register(&self, team: impl Into<String>, session: Arc<dyn BotSessionSource>) {
        self.connections.lock().unwrap().insert(team.into(), session);
    }

    /// Mint a fresh per-match driver for `team` WITHOUT removing the session.
    ///
    /// Returns `None` if no live session is registered for `team`.
    /// Called by the resolver at match-build time; the session persists so
    /// the same bot can play subsequent matches.
    pub fn make_driver(
        &self,
        team: &str,
        deadline: Duration,
        health: Option<Arc<BotHealthEntry>>,
    ) -> Option<Box<dyn BotDriver>> {
        self.connections
            .lock()
            .unwrap()
            .get(team)
            .map(|s| s.make_driver(deadline, health))
    }

    /// Remove the session for `team` (called on disconnect).
    pub fn remove(&self, team: &str) {
        self.connections.lock().unwrap().remove(team);
    }

    /// Return `true` if a live session is registered for `team`.
    pub fn has(&self, team: &str) -> bool {
        self.connections.lock().unwrap().contains_key(team)
    }

    /// Return all currently-connected team names.
    ///
    /// Used by the live-match orchestrator to build a default roster from
    /// whichever WS bots are connected at match-start time.
    pub fn connected_teams(&self) -> Vec<String> {
        self.connections.lock().unwrap().keys().cloned().collect()
    }

    /// Return a clone of the session for `team`, if present.
    ///
    /// Callers use this to send per-match envelope frames (matchStart /
    /// matchEnd) without going through the resolver.
    pub fn get(&self, team: &str) -> Option<Arc<dyn BotSessionSource>> {
        self.connections.lock().unwrap().get(team).cloned()
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
    /// Per-tick deadline passed to WS bot drivers.
    ///
    /// Controls how long `WsBotDriver::decide` waits for an action reply.
    /// Defaults to `100 ms` when constructed via [`ConnectionResolver::new`];
    /// set explicitly via [`ConnectionResolver::with_deadline`] or pass it
    /// from `AppState.tick_deadline` in production.
    deadline: Duration,
    /// Optional DQ store — checked before WS/WASM driver selection.
    /// When `None`, no exclusion is applied (backward-compat with tests).
    dq_store: Option<Arc<DqStore>>,
    /// Optional health store — a `BotHealthEntry` is registered per slot
    /// when this is `Some`.  When `None`, health is not tracked.
    health_store: Option<Arc<BotHealthStore>>,
    /// Optional disabled store — teams in this set resolve to Default Bot.
    /// When `None`, no disable check is applied (backward-compat with tests).
    disabled_store: Option<Arc<DisabledStore>>,
    /// Optional custom Default Bot artifact — when set, Priority-3 resolution
    /// attempts to instantiate a WASM driver from this artifact instead of the
    /// built-in heuristic.  When `None`, the built-in DefaultBotDriver is used.
    default_bot_store: Option<Arc<DefaultBotStore>>,
}

impl ConnectionResolver {
    /// Construct a resolver.
    ///
    /// - `ws_registry` — shared live-WS-connection registry (wired into the
    ///   WS handler by issue 11).
    /// - `wasm_store` — shared WASM artifact store (wired into `POST /bots`).
    /// - `fuel_per_tick` — wasmtime instruction budget per
    ///   [`WasmBotDriver`] tick call.
    ///
    /// The default per-tick deadline is `100 ms`; set it via
    /// [`ConnectionResolver::with_deadline`] for production use.
    pub fn new(
        ws_registry: Arc<WsConnectionRegistry>,
        wasm_store: Arc<WasmBotStore>,
        fuel_per_tick: u64,
    ) -> Self {
        Self {
            ws_registry,
            wasm_store,
            fuel_per_tick,
            deadline: Duration::from_millis(100),
            dq_store: None,
            health_store: None,
            disabled_store: None,
            default_bot_store: None,
        }
    }

    /// Set the per-tick deadline for WS bot drivers.
    ///
    /// Should be set to `AppState::tick_deadline` in production so the
    /// resolver-created drivers respect the same timing budget as the inline
    /// WS handler.
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
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

    /// Attach disable and default-bot management stores (issue 13).
    ///
    /// When set:
    /// - Disabled teams fall back to the Default Bot (reversible; re-enabling
    ///   restores normal WS → WASM → Default priority).
    /// - An uploaded Default Bot artifact is used instead of the built-in
    ///   heuristic when no team-specific WS/WASM driver is chosen.
    pub fn with_management(
        mut self,
        disabled: Arc<DisabledStore>,
        default_bot: Arc<DefaultBotStore>,
    ) -> Self {
        self.disabled_store = Some(disabled);
        self.default_bot_store = Some(default_bot);
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
    /// WS sessions remain in the registry after this call — the same bot
    /// can play subsequent matches without reconnecting.
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

        // If disable management is active and team is disabled → Default Bot
        // (reversible fallback; no ExclusionDriver wrapping so re-enabling
        // immediately restores WASM/WS resolution for the next match).
        if let Some(ds) = &self.disabled_store {
            if ds.is_disabled(&slot.team) {
                return self.make_default_bot(slot, params);
            }
        }

        // Priority 1 — WS Bot: mint a fresh per-match driver from the
        // persistent session WITHOUT removing it from the registry.
        let health_entry = self.make_health(&slot.team, "ws");
        if let Some(driver) = self.ws_registry.make_driver(
            &slot.team,
            self.deadline,
            health_entry.clone(),
        ) {
            // Wrap in ExclusionDriver for mid-match kick support.
            if let Some(dq) = &self.dq_store {
                let health = health_entry
                    .or_else(|| self.health_store.as_ref().and_then(|hs| hs.get(&slot.team)));
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

        // Priority 3 — Default Bot: built-in fallback (or custom WASM artifact).
        self.make_default_bot(slot, params)
    }

    /// Build the Default Bot driver for `slot`.
    ///
    /// If a custom Default Bot artifact is stored, attempt to instantiate a
    /// [`WasmBotDriver`] from it.  On failure (bad bytes, wasmtime error) fall
    /// silently back to the built-in [`DefaultBotDriver`] so the match never
    /// aborts.
    fn make_default_bot(&self, slot: &Slot, params: &Params) -> Box<dyn BotDriver> {
        // Try custom WASM default bot first.
        if let Some(dbs) = &self.default_bot_store {
            if let Some(bytes) = dbs.get() {
                if let Ok(driver) =
                    WasmBotDriver::new(&bytes, &slot.tick0_obs_json, self.fuel_per_tick)
                {
                    // Health tracking for the custom default bot.
                    let health = self.make_health(&slot.team, "wasm");
                    let mut driver = driver;
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
                // Fall through: bad artifact → built-in heuristic below.
            }
        }

        // Built-in heuristic fallback.
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
