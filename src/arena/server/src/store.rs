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

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
}
