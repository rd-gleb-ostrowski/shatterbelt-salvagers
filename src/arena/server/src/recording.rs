//! Match recording store (issue 08).
//!
//! Every finished match has its seed, scaled params, ship specs, and full
//! applied-intent log persisted in a [`RecordingStore`].  The store lives
//! in [`AppState`] as `Arc<RecordingStore>` so it is shared across the match
//! lifecycle and HTTP handlers.
//!
//! ## Replay guarantee
//!
//! `(seed, params, specs, intent_log)` is sufficient to reconstruct an
//! identical match via [`arena_engine::harness::replay_match`] — the engine
//! is deterministic given the same inputs.
//!
//! ## Seams for future issues
//!
//! | Future issue | Seam |
//! |---|---|
//! | 10 (TrueSkill ladder) | consume `RecordingMeta` (winner, scores) from `list()` |
//! | 11 (Admin UI)         | `GET /recordings` → `list()`; `POST /recordings/{id}/replay` |

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use arena_engine::{IntentFrame, Params, ShipId, ShipSpec};

// ── Recording ─────────────────────────────────────────────────────────────────

/// Everything needed to replay a finished match exactly, plus lightweight
/// metadata shown in the listing.
#[derive(Debug, Clone)]
pub struct Recording {
    /// Stable identifier for this recording (UUID v4).
    pub match_id: String,
    /// RNG seed used to construct the engine.
    pub seed: u64,
    /// The *already-scaled* `Params` used in this match.
    ///
    /// Must be passed as-is to [`arena_engine::harness::replay_match`].
    pub params: Params,
    /// Ship specs used to construct the engine.
    pub specs: Vec<ShipSpec>,
    /// Full applied-intent log — one [`IntentFrame`] per completed tick.
    pub intent_log: Vec<IntentFrame>,
    /// Lightweight metadata (winner, scores, tick count) for the listing.
    pub meta: RecordingMeta,
}

/// Lightweight per-recording metadata returned by [`RecordingStore::list`].
///
/// Intentionally a small, cloneable type so listing is cheap.
#[derive(Debug, Clone)]
pub struct RecordingMeta {
    /// Stable identifier for this recording (UUID v4).
    pub match_id: String,
    /// Seed the match was played with.
    pub seed: u64,
    /// Total ticks completed.
    pub tick_count: u32,
    /// Winning ship, or `None` on a tie.
    pub winner: Option<ShipId>,
    /// Final banked score per ship.
    pub scores: Vec<(ShipId, f32)>,
}

// ── RecordingStore ────────────────────────────────────────────────────────────

/// Shared in-memory store of finished-match recordings, keyed by `match_id`.
///
/// Create one instance at server startup with [`RecordingStore::new`] and
/// share it as `Arc<RecordingStore>`.
///
/// ## Seams for future issues
///
/// - **Issue 10 (TrueSkill):** iterate `list()` to pull winner + scores for
///   rating updates after each match finishes.
/// - **Issue 11 (Admin UI):** expose `list()` and `get(id)` via HTTP; the admin
///   endpoint can download or replay any stored recording.
#[derive(Debug, Default)]
pub struct RecordingStore {
    inner: RwLock<HashMap<String, Recording>>,
}

impl RecordingStore {
    /// Create a new, empty store wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Persist a finished-match recording.
    ///
    /// The `recording.match_id` is used as the key; callers should generate
    /// a UUID before constructing the [`Recording`].  If the same `match_id`
    /// is stored again the entry is replaced.
    pub fn record(&self, recording: Recording) {
        self.inner
            .write()
            .unwrap()
            .insert(recording.match_id.clone(), recording);
    }

    /// Retrieve the full [`Recording`] by `match_id`, if present.
    pub fn get(&self, match_id: &str) -> Option<Recording> {
        self.inner.read().unwrap().get(match_id).cloned()
    }

    /// List lightweight metadata for all stored recordings.
    ///
    /// Returns an unordered snapshot; callers may sort by any field as needed.
    pub fn list(&self) -> Vec<RecordingMeta> {
        self.inner
            .read()
            .unwrap()
            .values()
            .map(|r| r.meta.clone())
            .collect()
    }
}
