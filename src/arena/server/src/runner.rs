//! Core match-runner loop: traits, `MatchRunner`, and `MatchOutcome`.
//!
//! # Design for testability
//!
//! Two seams keep the match loop testable without real clocks or network:
//!
//! ## `BotDriver` — intent source abstraction
//!
//! Each ship is associated with one `BotDriver`. Every tick the runner:
//! 1. Reads the ship's `Observation` from the engine.
//! 2. Calls `driver.decide(tick, &obs)`.
//! 3. Collects only `Some(intent)` responses; ships that return `None` are
//!    omitted from the intent vec passed to `engine.step()`.
//!
//! The engine already implements per-field intent persistence (PROTOCOL §2 /
//! ADR-0003): an absent intent for a ship means its previous turn/thrust/fire
//! values persist. The server never needs to track "last intent" itself.
//!
//! **Future issues plug in here:**
//! - Issue 03 (WS bots): a WS driver reads from a channel and returns `None`
//!   when the deadline passes with no message.
//! - Issue 06 (connection resolver): wraps the WS → WASM → Default priority
//!   chain behind this trait.
//!
//! ## `TickPacer` — timing abstraction
//!
//! Live matches pace to ~30 Hz via `LivePacer`; tests use `NoopPacer`. The
//! pacer is only invoked by `run_to_completion`; `step_once` never sleeps, so
//! tests can call it directly to drive ticks one at a time.

use arena_engine::{Engine, Event, Intent, Observation, Params, ShipId, ShipSpec};

// ── Traits ─────────────────────────────────────────────────────────────────────

/// Provides the intent for one ship each tick.
///
/// The [`MatchRunner`] calls `decide` for each ship every tick by passing the
/// current tick number and the ship's [`Observation`].
///
/// - Return `Some(intent)` if a fresh intent is available before the per-tick
///   deadline.
/// - Return `None` to let the engine carry the ship's previous intent forward
///   (per-field persistence, PROTOCOL §2 / ADR-0003). The engine applies the
///   previously-merged turn/thrust/fire fields; the server does **not** need to
///   remember them itself.
///
/// # Seam
///
/// This trait is the primary plug-point for future issues:
/// - Issue 03 (WS bots): a driver that reads from a channel and returns `None`
///   when the per-tick deadline passes with no incoming message.
/// - Issue 06 (connection resolver): the WS → WASM → Default priority chain.
pub trait BotDriver: Send {
    fn decide(&mut self, tick: u32, obs: &Observation) -> Option<Intent>;

    /// A short label identifying the driver kind for observability.
    ///
    /// Canonical values: `"default"`, `"wasm"`, `"ws"`.
    ///
    /// The default implementation returns `"unknown"`; all concrete types
    /// shipped in this crate override it.  Tests and issue 12 (bot health)
    /// use this to distinguish driver kinds without downcasting.
    fn kind(&self) -> &'static str {
        "unknown"
    }
}

/// Abstracts tick pacing between calls to [`MatchRunner::step_once`].
///
/// - **Live match** ([`LivePacer`](crate::pacer::LivePacer)): sleeps to
///   maintain ~30 Hz (≈33 ms/tick, accounting for step duration). This is the
///   only place wall-clock time enters the server; the engine is always headless.
/// - **Tests / headless-fast** ([`NoopPacer`](crate::pacer::NoopPacer)): no-op.
///
/// [`MatchRunner::step_once`] does **not** invoke the pacer. Only
/// [`MatchRunner::run_to_completion`] does, so tests can call `step_once`
/// directly without any sleeping.
pub trait TickPacer: Send {
    fn wait_for_next_tick(&mut self);
}

// ── MatchOutcome ──────────────────────────────────────────────────────────────

/// Summary of a completed match.
///
/// Returned by [`MatchRunner::run_to_completion`]. For the full applied-intent
/// log and god-view needed by the recorder (issue 08), call
/// `runner.engine().intent_log()` and `runner.engine().god_view()` directly.
#[derive(Debug, Clone)]
pub struct MatchOutcome {
    /// The ship with the highest score, or `None` on a tie.
    pub winner: Option<ShipId>,
    /// Final banked score per ship.
    pub scores: Vec<(ShipId, f32)>,
    /// Total ticks elapsed (equals `params.max_ticks` for a full-length match).
    pub ticks: u32,
}

// ── MatchRunner ───────────────────────────────────────────────────────────────

/// Wraps an [`Engine`] with per-ship [`BotDriver`]s and a [`TickPacer`].
///
/// ## Match lifecycle
///
/// ```text
/// MatchRunner::new(seed, params, specs, drivers, pacer)
///     └── Engine::new(seed, params, specs)
///
/// loop until is_match_over():
///     step_once()
///         for each ship:
///             obs  = engine.observation(ship_id)
///             intent = driver.decide(tick, &obs)   // None → engine persists previous
///         engine.step(fresh_intents_only)
///     pacer.wait_for_next_tick()                   // skipped in test (NoopPacer)
/// ```
///
/// ## Seams for future issues
///
/// - **Issue 07 (observer)**: call `runner.engine().god_view()` after each tick.
/// - **Issue 08 (recorder)**: call `runner.engine().intent_log()` after match ends.
/// - **Issue 02 (registration)**: pass registered drivers into `new(...)`.
pub struct MatchRunner {
    engine: Engine,
    /// Parallel to the engine's ship list: `(ship_id, driver)`.
    drivers: Vec<(ShipId, Box<dyn BotDriver>)>,
    pacer: Box<dyn TickPacer>,
}

impl MatchRunner {
    /// Construct a runner from seed, params, specs, and one driver per ship.
    ///
    /// `specs` and `drivers` must have the same length; index `i` in `drivers`
    /// pilots the ship defined by `specs[i]`.
    ///
    /// # Panics
    ///
    /// Panics if `specs.len() != drivers.len()`.
    pub fn new(
        seed: u64,
        params: Params,
        specs: Vec<ShipSpec>,
        drivers: Vec<Box<dyn BotDriver>>,
        pacer: Box<dyn TickPacer>,
    ) -> Self {
        assert_eq!(
            specs.len(),
            drivers.len(),
            "one BotDriver per ShipSpec required"
        );
        let drivers: Vec<(ShipId, Box<dyn BotDriver>)> = specs
            .iter()
            .map(|s| s.id.clone())
            .zip(drivers)
            .collect();
        let engine = Engine::new(seed, params, specs);
        Self { engine, drivers, pacer }
    }

    /// Advance exactly one tick.
    ///
    /// For each ship:
    /// 1. Read its `Observation` from the engine.
    /// 2. Call the driver's `decide(tick, &obs)`.
    /// 3. Collect only `Some(intent)` responses into the intent vec.
    ///
    /// Ships whose driver returns `None` are absent from the intent vec.  The
    /// engine's per-field intent persistence (see `PersistedIntent` in the
    /// engine) applies their previous turn/thrust/fire values unchanged — the
    /// server does not track "last intent" itself.
    ///
    /// **Does not invoke the pacer.** Call from tests without sleeping.
    pub fn step_once(&mut self) -> Vec<(ShipId, Vec<Event>)> {
        let tick = self.engine.tick();
        let intents: Vec<(ShipId, Intent)> = self
            .drivers
            .iter_mut()
            .filter_map(|(ship_id, driver)| {
                let obs = self.engine.observation(ship_id)?;
                let intent = driver.decide(tick, &obs)?;
                Some((ship_id.clone(), intent))
            })
            .collect();
        self.engine.step(intents)
    }

    /// Run the match to completion, pacing ticks via the [`TickPacer`].
    ///
    /// Loops until `engine.is_match_over()`, calling `step_once` then
    /// `pacer.wait_for_next_tick()` each iteration.
    ///
    /// With a [`NoopPacer`](crate::pacer::NoopPacer) this runs at CPU speed
    /// (no sleeping) — suitable for tests and headless-fast ladder matches.
    /// With a [`LivePacer`](crate::pacer::LivePacer) it is paced to ~30 Hz.
    pub fn run_to_completion(&mut self) -> MatchOutcome {
        while !self.engine.is_match_over() {
            self.step_once();
            self.pacer.wait_for_next_tick();
        }
        let scores = self
            .drivers
            .iter()
            .map(|(id, _)| (id.clone(), self.engine.score(id).unwrap_or(0.0)))
            .collect();
        MatchOutcome {
            winner: self.engine.winner(),
            scores,
            ticks: self.engine.tick(),
        }
    }

    /// Borrow the underlying engine for inspection.
    ///
    /// Callers can use `engine.god_view()` (issue 07 observer),
    /// `engine.intent_log()` (issue 08 recorder), `engine.tick()`, etc.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}
