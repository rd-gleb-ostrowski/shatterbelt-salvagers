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
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use arena_engine::{IntentFrame, Params, ShipId, ShipSpec};

// ── Recording ─────────────────────────────────────────────────────────────────

/// Everything needed to replay a finished match exactly, plus lightweight
/// metadata shown in the listing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// Shared store of finished-match recordings, keyed by `match_id`.
///
/// Create one instance at server startup with [`RecordingStore::new`] (in-memory
/// only) or [`RecordingStore::with_dir`] (disk-backed) and share it as
/// `Arc<RecordingStore>`.
///
/// ## Persistence
///
/// When constructed via [`RecordingStore::with_dir`]:
/// - Existing `*.json` files in the directory are loaded into memory on
///   construction (survives a server restart).
/// - Each call to [`record`](RecordingStore::record) additionally writes a
///   `{match_id}.json` file to the directory.
/// - Disk I/O failures are logged (via `eprintln!`) but never panic — the
///   match result is always stored in memory regardless.
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
    /// Optional persistence directory. When `Some`, recordings are written as
    /// `{match_id}.json` on every [`record`](RecordingStore::record) call.
    dir: Option<PathBuf>,
}

impl RecordingStore {
    /// Create a new, empty in-memory store wrapped in `Arc`.
    ///
    /// No disk I/O is performed. Recordings survive only for the lifetime of
    /// the process.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Create a disk-backed store rooted at `path`, wrapped in `Arc`.
    ///
    /// On construction:
    /// 1. The directory is created (including parents) if it does not exist.
    /// 2. All `*.json` files present in the directory are deserialised and
    ///    loaded into the in-memory map — recordings from a previous server
    ///    run are immediately available.
    ///
    /// Failures (unreadable dir, malformed JSON) are reported via `eprintln!`
    /// and skipped; the store is still usable.
    pub fn with_dir(path: PathBuf) -> Arc<Self> {
        if let Err(e) = std::fs::create_dir_all(&path) {
            eprintln!(
                "[arena-server] WARNING: could not create recordings dir {:?}: {e}",
                path
            );
        }

        let store = Arc::new(Self {
            inner: RwLock::new(HashMap::new()),
            dir: Some(path.clone()),
        });

        match std::fs::read_dir(&path) {
            Err(e) => {
                eprintln!(
                    "[arena-server] WARNING: could not read recordings dir {:?}: {e}",
                    path
                );
            }
            Ok(entries) => {
                let mut map = store.inner.write().unwrap();
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.extension().is_some_and(|ext| ext == "json") {
                        match std::fs::read_to_string(&entry_path) {
                            Err(e) => eprintln!(
                                "[arena-server] WARNING: could not read recording {:?}: {e}",
                                entry_path
                            ),
                            Ok(json) => match serde_json::from_str::<Recording>(&json) {
                                Err(e) => eprintln!(
                                    "[arena-server] WARNING: could not parse recording {:?}: {e}",
                                    entry_path
                                ),
                                Ok(rec) => {
                                    map.insert(rec.match_id.clone(), rec);
                                }
                            },
                        }
                    }
                }
            }
        }

        store
    }

    /// Persist a finished-match recording (memory + optional disk).
    ///
    /// The `recording.match_id` is used as the key; callers should generate
    /// a UUID before constructing the [`Recording`].  If the same `match_id`
    /// is stored again the entry is replaced.
    ///
    /// When a persistence directory is configured, the recording is also
    /// written as `{match_id}.json`.  Disk failures are logged, never panicked.
    pub fn record(&self, recording: Recording) {
        if let Some(dir) = &self.dir {
            let file_path = dir.join(format!("{}.json", recording.match_id));
            match serde_json::to_string(&recording) {
                Err(e) => eprintln!(
                    "[arena-server] WARNING: could not serialise recording {}: {e}",
                    recording.match_id
                ),
                Ok(json) => {
                    if let Err(e) = std::fs::write(&file_path, json) {
                        eprintln!(
                            "[arena-server] WARNING: could not write recording to {:?}: {e}",
                            file_path
                        );
                    }
                }
            }
        }
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
