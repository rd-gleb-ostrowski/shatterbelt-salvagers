//! Headless-fast match runner (issue 09).
//!
//! Executes matches at CPU speed (no pacing) using **WASM Bots and Default
//! Bots only**.  WS Bots are excluded by always constructing a fresh empty
//! [`WsConnectionRegistry`] for every match — the [`ConnectionResolver`] never
//! finds a WS entry and falls through to WASM or Default.
//!
//! ## Public API
//!
//! | Method | Purpose |
//! |--------|---------|
//! | [`HeadlessRunner::run_one`] | Run one match (auto-increment seed); returns [`HeadlessResult`]. |
//! | [`HeadlessRunner::run_one_seeded`] | Like `run_one` but with an explicit seed (deterministic tests). |
//! | [`HeadlessRunner::run_n`] | Async helper: run exactly `n` matches into a channel. Never hangs. |
//! | [`HeadlessRunner::spawn_loop`] | Spawn a background task that runs matches continuously until stopped. |
//!
//! ## WS exclusion
//!
//! `run_one_seeded` always constructs a **new empty [`WsConnectionRegistry`]**
//! and passes it to the [`ConnectionResolver`].  Any external registry with
//! live WS drivers is never referenced, so WS is structurally impossible in
//! headless matches regardless of what teams are registered.
//!
//! ## Seams for future issues
//!
//! | Future issue | Seam |
//! |---|---|
//! | 10 (TrueSkill ladder) | Read [`HeadlessResult`]s from the `mpsc::Receiver` returned by [`HeadlessRunner::spawn_loop`]. |
//! | 11 (admin start/pause) | Hold the `watch::Sender<bool>` from `spawn_loop`; call `stop_tx.send(true)` to halt the loop. |

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use arena_engine::{Engine, Params, ShipSpec};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::pacer::NoopPacer;
use crate::recording::{Recording, RecordingMeta, RecordingStore};
use crate::resolver::{ConnectionResolver, Slot, WsConnectionRegistry};
use crate::runner::{MatchOutcome, MatchRunner};
use crate::store::{DefaultBotStore, DisabledStore, WasmBotStore};
use crate::ws::obs_to_tick_json;

// ── HeadlessResult ────────────────────────────────────────────────────────────

/// Result of a single completed headless match.
///
/// The full intent log and replay data are persisted in the
/// [`RecordingStore`] under [`HeadlessResult::match_id`].  Issue 10 reads
/// `outcome.scores` to update the TrueSkill ladder.
#[derive(Debug, Clone)]
pub struct HeadlessResult {
    /// Stable identifier for this match (UUID v4); also the key in the
    /// [`RecordingStore`].
    pub match_id: String,
    /// RNG seed used for this match.
    pub seed: u64,
    /// Winner, per-ship scores, and tick count.
    pub outcome: MatchOutcome,
    /// [`BotDriver::kind`](crate::runner::BotDriver::kind) for each slot in
    /// the same order as the [`HeadlessRunner`]'s `teams` list.
    ///
    /// Canonical values: `"default"`, `"wasm"`.  `"ws"` must never appear in
    /// a headless result — that would indicate a bug in WS exclusion.
    ///
    /// Tests assert `driver_kinds` to verify WS exclusion (issue 09 AC).
    pub driver_kinds: Vec<String>,
}

// ── HeadlessRunner ────────────────────────────────────────────────────────────

/// Drives headless-fast matches in the background.
///
/// Constructed once at server startup and shared as `Arc<HeadlessRunner>`.
/// Use [`HeadlessRunner::new`] to create.
///
/// ## Thread safety
///
/// All shared state is behind `Arc` or `Atomic` types; `HeadlessRunner` is
/// `Send + Sync` and can be referenced from multiple async tasks.
pub struct HeadlessRunner {
    wasm_store: Arc<WasmBotStore>,
    recording_store: Arc<RecordingStore>,
    params: Params,
    specs: Vec<ShipSpec>,
    teams: Vec<String>,
    fuel_per_tick: u64,
    /// Monotonically-increasing match seed.  Each `run_one` call atomically
    /// increments this so successive matches use distinct seeds.
    seed_counter: AtomicU64,
    /// Optional DQ store (issue 12): DQ'd teams fall back to Default Bot.
    dq_store: Option<Arc<crate::health::DqStore>>,
    /// Optional health store (issue 12): health entries are created per slot.
    health_store: Option<Arc<crate::health::BotHealthStore>>,
    /// Optional disabled store (issue 13): disabled teams fall back to Default Bot.
    disabled_store: Option<Arc<DisabledStore>>,
    /// Optional custom Default Bot artifact (issue 13).
    default_bot_store: Option<Arc<DefaultBotStore>>,
}

impl HeadlessRunner {
    /// Construct a headless runner, returning it wrapped in `Arc`.
    ///
    /// # Parameters
    ///
    /// - `wasm_store` — shared WASM artifact store populated by `POST /bots`.
    /// - `recording_store` — each finished match is persisted here.
    /// - `params` — match parameters (tick count, arena dimensions, …).
    /// - `specs` — one [`ShipSpec`] per team slot, parallel to `teams`.
    /// - `teams` — team identities parallel to `specs`.
    /// - `fuel_per_tick` — wasmtime instruction budget per WASM bot tick call.
    /// - `base_seed` — initial seed; each successive match uses
    ///   `base_seed + match_index`.
    ///
    /// # Panics
    ///
    /// Panics if `specs.len() != teams.len()`.
    pub fn new(
        wasm_store: Arc<WasmBotStore>,
        recording_store: Arc<RecordingStore>,
        params: Params,
        specs: Vec<ShipSpec>,
        teams: Vec<String>,
        fuel_per_tick: u64,
        base_seed: u64,
    ) -> Arc<Self> {
        assert_eq!(specs.len(), teams.len(), "one team per spec required");
        Arc::new(Self {
            wasm_store,
            recording_store,
            params,
            specs,
            teams,
            fuel_per_tick,
            seed_counter: AtomicU64::new(base_seed),
            dq_store: None,
            health_store: None,
            disabled_store: None,
            default_bot_store: None,
        })
    }

    /// Construct a headless runner with shared DQ and health stores (issue 12).
    ///
    /// Identical to [`new`](Self::new) but the runner will:
    /// - Skip WS/WASM bots for DQ'd teams (Default Bot fills the slot).
    /// - Create a `BotHealthEntry` per slot and update it each tick.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_health(
        wasm_store: Arc<WasmBotStore>,
        recording_store: Arc<RecordingStore>,
        params: Params,
        specs: Vec<ShipSpec>,
        teams: Vec<String>,
        fuel_per_tick: u64,
        base_seed: u64,
        dq_store: Arc<crate::health::DqStore>,
        health_store: Arc<crate::health::BotHealthStore>,
    ) -> Arc<Self> {
        assert_eq!(specs.len(), teams.len(), "one team per spec required");
        Arc::new(Self {
            wasm_store,
            recording_store,
            params,
            specs,
            teams,
            fuel_per_tick,
            seed_counter: AtomicU64::new(base_seed),
            dq_store: Some(dq_store),
            health_store: Some(health_store),
            disabled_store: None,
            default_bot_store: None,
        })
    }

    /// Attach management stores (issue 13) to an already-constructed runner.
    ///
    /// Consumes and returns `self` (builder pattern), so call after
    /// [`new_with_health`](Self::new_with_health) before running matches.
    pub fn with_management(
        mut self: Arc<Self>,
        disabled: Arc<DisabledStore>,
        default_bot: Arc<DefaultBotStore>,
    ) -> Arc<Self> {
        // SAFETY: Arc::get_mut succeeds when there is exactly one strong
        // reference, which is guaranteed here because we just constructed self
        // and haven't shared it yet.
        if let Some(inner) = Arc::get_mut(&mut self) {
            inner.disabled_store = Some(disabled);
            inner.default_bot_store = Some(default_bot);
        }
        self
    }

    /// Run one headless match, advancing the internal seed counter.
    ///
    /// WS Bots are **always excluded** — a fresh empty
    /// [`WsConnectionRegistry`] is constructed for every call so the
    /// [`ConnectionResolver`] never finds a WS entry.
    ///
    /// The finished match is persisted to the [`RecordingStore`].
    pub fn run_one(&self) -> HeadlessResult {
        let seed = self.seed_counter.fetch_add(1, Ordering::Relaxed);
        self.run_one_seeded(seed)
    }

    /// Run one headless match with an explicit seed.
    ///
    /// Useful in deterministic tests where the caller controls the seed.
    /// Advances neither the internal counter nor changes external state.
    ///
    /// WS exclusion guarantee is identical to [`run_one`](Self::run_one):
    /// a brand-new empty [`WsConnectionRegistry`] is always used.
    pub fn run_one_seeded(&self, seed: u64) -> HeadlessResult {
        // ── WS exclusion: fresh empty registry, never touched externally ──────
        let empty_ws = WsConnectionRegistry::new();
        let mut resolver = ConnectionResolver::new(
            Arc::clone(&empty_ws),
            Arc::clone(&self.wasm_store),
            self.fuel_per_tick,
        );

        // Wire moderation if stores are present (issue 12).
        if let (Some(dq), Some(hs)) = (self.dq_store.as_ref(), self.health_store.as_ref()) {
            resolver = resolver.with_moderation(Arc::clone(dq), Arc::clone(hs));
        }

        // Wire management stores if present (issue 13).
        if let (Some(ds), Some(dbs)) =
            (self.disabled_store.as_ref(), self.default_bot_store.as_ref())
        {
            resolver = resolver.with_management(Arc::clone(ds), Arc::clone(dbs));
        }

        // Build a temporary engine solely to extract tick-0 observations for
        // WASM bot warm-up (ADR-0004).  The real match engine is created
        // inside MatchRunner::new.
        let engine0 = Engine::new(seed, self.params.clone(), self.specs.clone());
        let slots: Vec<Slot> = self
            .teams
            .iter()
            .zip(self.specs.iter())
            .map(|(team, spec)| {
                let tick0_obs_json = engine0
                    .observation(&spec.id)
                    .map(|obs| obs_to_tick_json(0, &obs))
                    .unwrap_or_default();
                Slot { team: team.clone(), tick0_obs_json }
            })
            .collect();

        let drivers = resolver.resolve(&slots, &self.params);

        // Capture driver kinds BEFORE ownership moves into MatchRunner.
        let driver_kinds: Vec<String> =
            drivers.iter().map(|d| d.kind().to_string()).collect();

        // NoopPacer → uncapped, CPU-speed execution.
        let mut runner = MatchRunner::new(
            seed,
            self.params.clone(),
            self.specs.clone(),
            drivers,
            Box::new(NoopPacer),
        );

        let outcome = runner.run_to_completion();
        let match_id = Uuid::new_v4().to_string();

        // Persist the recording so issue 10 / issue 11 can replay or audit.
        let meta = RecordingMeta {
            match_id: match_id.clone(),
            seed,
            tick_count: outcome.ticks,
            winner: outcome.winner.clone(),
            scores: outcome.scores.clone(),
        };
        self.recording_store.record(Recording {
            match_id: match_id.clone(),
            seed,
            params: self.params.clone(),
            specs: self.specs.clone(),
            intent_log: runner.engine().intent_log().to_vec(),
            meta,
        });

        HeadlessResult { match_id, seed, outcome, driver_kinds }
    }

    /// Run exactly `count` headless matches, sending each [`HeadlessResult`]
    /// into `tx`.
    ///
    /// Returns when all matches are done or when `tx` is closed (receiver
    /// dropped).  Safe to use in tests — **never hangs** because the loop is
    /// bounded by `count`.
    ///
    /// Each match is run on a blocking thread so the async executor stays
    /// responsive.
    pub async fn run_n(self: Arc<Self>, count: usize, tx: mpsc::Sender<HeadlessResult>) {
        for _ in 0..count {
            let runner = Arc::clone(&self);
            let result = tokio::task::spawn_blocking(move || runner.run_one())
                .await
                .expect("headless match task panicked");
            if tx.send(result).await.is_err() {
                break; // Receiver dropped — stop gracefully.
            }
        }
    }

    /// Spawn a background task that runs matches **continuously** until a stop
    /// signal is received.
    ///
    /// Returns `(stop_tx, result_rx, handle)`:
    ///
    /// - **`stop_tx`** — a [`watch::Sender<bool>`] owned by the caller
    ///   (issue 11 admin).  Call `stop_tx.send(true)` to request a graceful
    ///   halt.  The loop finishes the current in-flight match and then exits.
    /// - **`result_rx`** — an [`mpsc::Receiver<HeadlessResult>`] that issue 10
    ///   (TrueSkill ladder) reads from.  The loop also stops when the receiver
    ///   is dropped (channel closed).
    /// - **`handle`** — a [`tokio::task::JoinHandle<()>`]; `await` it to
    ///   confirm the loop has finished.
    ///
    /// ## Stop semantics
    ///
    /// The stop signal is checked once per match, just before dispatching the
    /// next `spawn_blocking` call.  A match already in flight always runs to
    /// completion — headless matches are short (milliseconds) so partial
    /// truncation is never needed.
    pub fn spawn_loop(
        self: Arc<Self>,
    ) -> (
        watch::Sender<bool>,
        mpsc::Receiver<HeadlessResult>,
        tokio::task::JoinHandle<()>,
    ) {
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let (tx, rx) = mpsc::channel(64);

        let handle = tokio::spawn(async move {
            loop {
                // Check stop signal before starting the next match.
                if *stop_rx.borrow() {
                    break;
                }

                let runner = Arc::clone(&self);
                let result = tokio::task::spawn_blocking(move || runner.run_one())
                    .await
                    .expect("headless match task panicked");

                if tx.send(result).await.is_err() {
                    break; // Receiver dropped.
                }

                // Re-check stop signal so a send that arrived during the
                // blocking match is respected immediately on the next iteration.
                if stop_rx.has_changed().unwrap_or(false) && *stop_rx.borrow_and_update() {
                    break;
                }
            }
        });

        (stop_tx, rx, handle)
    }
}
