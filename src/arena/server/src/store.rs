//! WASM Bot artifact store (issue 04).
//!
//! [`WasmBotStore`] holds the latest compiled WASM artifact for each team.
//! It is the seam that connects:
//!
//! - **Issue 04 (this):** `POST /bots` writes via [`WasmBotStore::store`].
//! - **Issue 05 (wasmtime host):** fetches bytes via [`WasmBotStore::get`] to
//!   instantiate the module before a match.
//! - **Issue 06 (connection resolver):** calls [`WasmBotStore::get`] to decide
//!   whether a team has a WASM Bot (slot priority: WS → WASM → Default).
//! - **Issue 11 (Admin):** can call [`WasmBotStore::store`] on behalf of a team
//!   to upload / replace a WASM Bot via the facilitator-gated endpoint.
//!
//! # Re-upload semantics
//!
//! Calling [`WasmBotStore::store`] for a team that already has an artifact
//! **replaces** it atomically.  The previous bytes are discarded.
//!
//! # Thread safety
//!
//! The store is `Send + Sync` and should be shared as `Arc<WasmBotStore>`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

// ── WasmBotStore ──────────────────────────────────────────────────────────────

/// Per-team storage for compiled WASM Bot artifacts.
///
/// Create one instance at server startup with [`WasmBotStore::new`] and share
/// it across all handlers (and later, the wasmtime host) as
/// `Arc<WasmBotStore>`.
///
/// ## Seams
///
/// | Future issue | Usage |
/// |---|---|
/// | 05 (wasmtime host)       | `get(team)` to fetch the artifact for instantiation |
/// | 06 (connection resolver) | `get(team).is_some()` to decide WS → WASM → Default |
/// | 11 (Admin upload)        | `store(team, bytes)` from the facilitator endpoint |
#[derive(Debug, Default)]
pub struct WasmBotStore {
    /// team identity → latest WASM artifact bytes
    artifacts: RwLock<HashMap<String, Vec<u8>>>,
}

impl WasmBotStore {
    /// Create a new, empty store wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Store (or replace) the WASM artifact for `team`.
    ///
    /// A subsequent call for the same `team` **atomically replaces** the
    /// previous artifact.  This lets participants re-upload during the event.
    pub fn store(&self, team: &str, bytes: Vec<u8>) {
        self.artifacts.write().unwrap().insert(team.to_owned(), bytes);
    }

    /// Retrieve the stored WASM artifact for `team`, if any.
    ///
    /// Returns `None` when no artifact has been uploaded yet.
    /// The bytes are cloned out of the store so the caller owns them.
    pub fn get(&self, team: &str) -> Option<Vec<u8>> {
        self.artifacts.read().unwrap().get(team).cloned()
    }

    pub fn stored_teams(&self)-> Vec<String> {
        self.artifacts.read().unwrap().keys().cloned().collect()
    }
}

// ── DisabledStore ─────────────────────────────────────────────────────────────

/// Reversible set of disabled team names.
///
/// A disabled team's slot falls back to the Default Bot in match resolution.
/// Re-enabling restores normal WS → WASM → Default priority.
///
/// Unlike [`crate::health::DqStore`] (permanent per-event disqualification),
/// `DisabledStore` is fully reversible: the facilitator can toggle bots on/off
/// at any time between matches.
///
/// Lives in [`crate::routes::AppState`] as `Arc<DisabledStore>`.
pub struct DisabledStore {
    disabled: Mutex<HashSet<String>>,
}

impl DisabledStore {
    /// Create a new, empty store wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            disabled: Mutex::new(HashSet::new()),
        })
    }

    /// Mark `team` as disabled — their slot resolves to the Default Bot.
    pub fn disable(&self, team: &str) {
        self.disabled.lock().unwrap().insert(team.to_owned());
    }

    /// Remove the disabled flag for `team`, restoring normal resolution.
    pub fn enable(&self, team: &str) {
        self.disabled.lock().unwrap().remove(team);
    }

    /// Returns `true` if `team` is currently disabled.
    pub fn is_disabled(&self, team: &str) -> bool {
        self.disabled.lock().unwrap().contains(team)
    }
}

// ── DefaultBotStore ───────────────────────────────────────────────────────────

/// Optional WASM artifact to use as the Default Bot fallback.
///
/// When set, the resolver's Priority-3 (Default Bot) path instantiates a
/// [`crate::wasm_host::WasmBotDriver`] from this artifact instead of the
/// built-in [`crate::bot::DefaultBotDriver`] heuristic.  On instantiation
/// failure the built-in driver is used so a match is never aborted.
///
/// Lives in [`crate::routes::AppState`] as `Arc<DefaultBotStore>`.
pub struct DefaultBotStore {
    artifact: RwLock<Option<Vec<u8>>>,
}

impl DefaultBotStore {
    /// Create a new, empty store (no custom Default Bot) wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            artifact: RwLock::new(None),
        })
    }

    /// Store (or replace) the custom Default Bot artifact.
    pub fn set(&self, bytes: Vec<u8>) {
        *self.artifact.write().unwrap() = Some(bytes);
    }

    /// Retrieve the current artifact, if any.
    pub fn get(&self) -> Option<Vec<u8>> {
        self.artifact.read().unwrap().clone()
    }

    /// Clear the custom Default Bot — future matches use the built-in heuristic.
    pub fn clear(&self) {
        *self.artifact.write().unwrap() = None;
    }
}
