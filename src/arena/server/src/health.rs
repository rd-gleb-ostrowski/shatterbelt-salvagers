//! Bot health tracking and moderation (issue 12).
//!
//! ## Public API
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`BotHealthEntry`] | Per-bot atomically-updated health state |
//! | [`BotHealthStore`] | Event-scoped registry of all `BotHealthEntry` values |
//! | [`BotHealthSnapshot`] | JSON-serialisable snapshot for `GET /admin/bots` |
//! | [`DqStore`] | Persistent disqualification set; checked by [`ExclusionDriver`] |
//! | [`ExclusionDriver`] | Wraps any [`BotDriver`] — returns `None` when the team is DQ'd |
//!
//! ## Wiring
//!
//! ```text
//!  resolver (match start)
//!    │  creates Arc<BotHealthEntry>
//!    │  registers in BotHealthStore
//!    │  injects into WasmBotDriver / WsBotDriver  ← drivers update atomics
//!    │  wraps driver in ExclusionDriver            ← checks DqStore each tick
//!    ▼
//!  running match loop
//!    │  ExclusionDriver.decide():
//!    │    if team in DqStore → return None (connected=false)
//!    │    else              → delegate to inner driver
//!    ▼
//!  GET /admin/bots  ← reads health_store.list_snapshots()
//!  POST /admin/bots/{team}/kick  ← DqStore.disqualify(team) + health.set_connected(false)
//! ```

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
    time::SystemTime,
};

use arena_engine::{Intent, Observation};
use serde::Serialize;

use crate::runner::BotDriver;

// ── BotHealthEntry ─────────────────────────────────────────────────────────────

/// Atomically-updated health state for one bot slot.
///
/// Constructed by the resolver (or the WS handler) and injected into the
/// concrete driver so the driver can update it each tick.  The admin HTTP plane
/// reads consistent snapshots via [`BotHealthEntry::snapshot`].
pub struct BotHealthEntry {
    /// Team identity (matches the key in the resolver / ws-registry).
    pub team: String,
    /// Driver kind: `"ws"`, `"wasm"`, or `"default"`.
    pub kind: String,
    connected: AtomicBool,
    skipped_ticks: AtomicU64,
    crashes: AtomicU64,
    /// Unix timestamp in milliseconds; `−1` means never seen.
    last_seen_ms: AtomicI64,
    /// Recent log output (capped at 64 KiB).
    logs: Mutex<Vec<u8>>,
}

impl BotHealthEntry {
    pub fn new(team: impl Into<String>, kind: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            team: team.into(),
            kind: kind.into(),
            connected: AtomicBool::new(true),
            skipped_ticks: AtomicU64::new(0),
            crashes: AtomicU64::new(0),
            last_seen_ms: AtomicI64::new(-1),
            logs: Mutex::new(Vec::new()),
        })
    }

    pub fn set_connected(&self, v: bool) {
        self.connected.store(v, Ordering::SeqCst);
    }

    /// Record that the bot missed a deadline / exhausted fuel / was excluded.
    pub fn increment_skipped(&self) {
        self.skipped_ticks.fetch_add(1, Ordering::Relaxed);
    }

    /// Set absolute crash count (sum of fuel-exhausted + trap counters).
    pub fn set_crashes(&self, n: u64) {
        self.crashes.store(n, Ordering::Relaxed);
    }

    /// Record current wall-clock time as "last seen" (bot responded in time).
    pub fn touch(&self) {
        let ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.last_seen_ms.store(ms, Ordering::Relaxed);
    }

    /// Append raw bytes to the rolling log buffer (capped at 64 KiB).
    pub fn append_logs(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut guard = self.logs.lock().unwrap();
        guard.extend_from_slice(bytes);
        const MAX_LOG_BYTES: usize = 65536;
        let len = guard.len();
        if len > MAX_LOG_BYTES {
            guard.drain(..len - MAX_LOG_BYTES);
        }
    }

    /// Take a consistent snapshot for the admin HTTP response.
    pub fn snapshot(&self) -> BotHealthSnapshot {
        let last_seen_ms = self.last_seen_ms.load(Ordering::Relaxed);
        let logs = self.logs.lock().unwrap();
        BotHealthSnapshot {
            team: self.team.clone(),
            kind: self.kind.clone(),
            connected: self.connected.load(Ordering::SeqCst),
            last_seen: (last_seen_ms >= 0).then_some(last_seen_ms as u64),
            skipped_ticks: self.skipped_ticks.load(Ordering::Relaxed),
            crashes: self.crashes.load(Ordering::Relaxed),
            recent_logs: String::from_utf8_lossy(&logs).into_owned(),
        }
    }
}

// ── BotHealthSnapshot ──────────────────────────────────────────────────────────

/// JSON shape returned by `GET /admin/bots`.
///
/// All counts are monotonic within one event (they are never reset).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotHealthSnapshot {
    /// Team name.
    pub team: String,
    /// Driver kind: `"ws"`, `"wasm"`, or `"default"`.
    pub kind: String,
    /// `true` while the bot is connected / active.
    pub connected: bool,
    /// Unix timestamp (ms) of the last successful tick response, or `null`.
    pub last_seen: Option<u64>,
    /// Number of ticks where no intent was produced (deadline miss / fuel
    /// exhaustion / exclusion).
    pub skipped_ticks: u64,
    /// Total fault count (fuel exhaustions + non-fuel WASM traps).
    pub crashes: u64,
    /// Recent log output captured from the WASM `log` import (or WS bot
    /// side-channel), UTF-8 where possible (replacement chars for non-UTF-8).
    pub recent_logs: String,
}

// ── BotHealthStore ─────────────────────────────────────────────────────────────

/// Event-scoped registry of per-bot health entries, keyed by team name.
///
/// Lives in [`AppState`] as `Arc<BotHealthStore>`.
pub struct BotHealthStore {
    entries: Mutex<HashMap<String, Arc<BotHealthEntry>>>,
}

impl BotHealthStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(HashMap::new()),
        })
    }

    /// Register (or replace) the health entry for a team.
    ///
    /// Returns the (new) `Arc<BotHealthEntry>` for injection into the driver.
    pub fn register(&self, entry: Arc<BotHealthEntry>) -> Arc<BotHealthEntry> {
        let mut guard = self.entries.lock().unwrap();
        guard.insert(entry.team.clone(), Arc::clone(&entry));
        entry
    }

    pub fn get(&self, team: &str) -> Option<Arc<BotHealthEntry>> {
        self.entries.lock().unwrap().get(team).cloned()
    }

    /// Snapshot all entries, sorted by team for deterministic ordering.
    pub fn list_snapshots(&self) -> Vec<BotHealthSnapshot> {
        let guard = self.entries.lock().unwrap();
        let mut snaps: Vec<_> = guard.values().map(|e| e.snapshot()).collect();
        snaps.sort_by(|a, b| a.team.cmp(&b.team));
        snaps
    }
}

// ── DqStore ────────────────────────────────────────────────────────────────────

/// Persistent set of disqualified team names.
///
/// Lives in [`AppState`] as `Arc<DqStore>`.
/// - The kick/DQ endpoint adds teams here.
/// - [`ExclusionDriver`] checks this on every tick.
/// - The resolver skips WS/WASM bots for DQ'd teams (uses Default Bot instead).
pub struct DqStore {
    disqualified: Mutex<HashSet<String>>,
}

impl DqStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            disqualified: Mutex::new(HashSet::new()),
        })
    }

    /// Permanently disqualify `team` for this event.
    pub fn disqualify(&self, team: &str) {
        self.disqualified.lock().unwrap().insert(team.to_owned());
    }

    /// Returns `true` if `team` has been disqualified.
    pub fn is_disqualified(&self, team: &str) -> bool {
        self.disqualified.lock().unwrap().contains(team)
    }
}

// ── ExclusionDriver ────────────────────────────────────────────────────────────

/// Wraps any [`BotDriver`] and short-circuits to `None` when the team is DQ'd.
///
/// The `Arc<DqStore>` is shared across the entire event, so a kick applied
/// via the admin endpoint takes effect on the **next** call to `decide` — no
/// message passing or restarts required.
///
/// The wrapped driver's health entry is updated (`set_connected(false)`) on
/// the first tick after disqualification.
pub struct ExclusionDriver {
    team: String,
    inner: Box<dyn BotDriver>,
    dq_store: Arc<DqStore>,
    health: Option<Arc<BotHealthEntry>>,
}

impl ExclusionDriver {
    pub fn new(
        team: impl Into<String>,
        inner: Box<dyn BotDriver>,
        dq_store: Arc<DqStore>,
        health: Option<Arc<BotHealthEntry>>,
    ) -> Self {
        Self { team: team.into(), inner, dq_store, health }
    }
}

impl BotDriver for ExclusionDriver {
    fn kind(&self) -> &'static str {
        self.inner.kind()
    }

    fn decide(&mut self, tick: u32, obs: &Observation) -> Option<Intent> {
        if self.dq_store.is_disqualified(&self.team) {
            if let Some(h) = &self.health {
                h.set_connected(false);
            }
            return None;
        }
        self.inner.decide(tick, obs)
    }
}
