//! Integration tests for the headless-fast match runner (issue 09).
//!
//! ## Design
//!
//! All tests assert **observable behaviour** through the public API only — no
//! private field inspection.  Driver kind is observed via
//! [`HeadlessResult::driver_kinds`], which mirrors `BotDriver::kind()`.
//!
//! A tiny WAT WASM bot (the same `CONST_ACTION_WAT` pattern from
//! `resolver_tests`) fills the WASM slots.  All matches use `NoopPacer`
//! (via `HeadlessRunner::run_one_seeded`) so they run at CPU speed.
//!
//! ## TDD order (RED → GREEN for each before the next)
//!
//! 1. `headless_match_completes_with_wasm_and_default`
//!    — Run one seeded headless match with one WASM slot + one Default slot.
//!    Outcome must be valid (ticks == max_ticks, scores for both ships).
//!
//! 2. `headless_match_is_uncapped_completes_full_length_promptly`
//!    — A full-length (3 600-tick) match must complete in ≪ live time.
//!    Assert `outcome.ticks == 3 600` and wall-clock elapsed < 10 s.
//!    (Live pace: 120 s.  NoopPacer: typically < 1 s.)
//!
//! 3. `ws_driver_never_used_in_headless_run`
//!    — Even when a WASM bot is uploaded for a team AND a "ws"-kind stub
//!    is conceptually available, the headless runner always uses an empty
//!    WsConnectionRegistry → no "ws" kind ever appears in driver_kinds.
//!
//! 4. `continuous_runner_produces_n_results_then_stops_on_signal`
//!    — `spawn_loop` produces N results into the channel; sending `true` on
//!    the stop sender halts the loop.  Bounded, no hang.
//!
//! 5. `headless_result_carries_full_ranking`
//!    — `outcome.scores` contains one entry per ship; `outcome.winner` is
//!    `Some` or `None` (either is valid; presence is asserted).
//!
//! 6. `headless_match_is_recorded_to_store`
//!    — After `run_one_seeded`, the `RecordingStore` contains the match;
//!    `store.get(match_id)` returns a `Recording` with matching seed and
//!    a non-empty `intent_log`.

use std::sync::Arc;
use std::time::Instant;

use arena_engine::{Params, ShipClass, ShipSpec, Vec2};
use arena_server::{
    headless::{HeadlessResult, HeadlessRunner},
    recording::RecordingStore,
    store::WasmBotStore,
};

// ── WAT fixture — same pattern as resolver_tests ─────────────────────────────

/// A minimal WASM bot that always returns `{"thrust":1.0}`.
const CONST_ACTION_WAT: &str = r#"
(module
  (import "env" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 256) "{\"thrust\":1.0}")
  (func (export "alloc") (param $len i32) (result i32)
    i32.const 512
  )
  (func (export "init") (param $ptr i32) (param $len i32))
  (func (export "tick") (param $ptr i32) (param $len i32) (result i64)
    i64.const 256
    i64.const 32
    i64.shl
    i64.const 14
    i64.or
  )
)
"#;

fn wat_to_wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("WAT assembly failed")
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Fast match params: short tick count so tests finish in milliseconds.
fn fast_params(max_ticks: u32) -> Params {
    Params { max_ticks, ..Params::default() }
}

/// Two ships placed at opposite sides of the arena.
fn two_specs(params: &Params) -> Vec<ShipSpec> {
    vec![
        ShipSpec {
            id: "ship-0".to_owned(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.25, params.arena_h * 0.5),
        },
        ShipSpec {
            id: "ship-1".to_owned(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2::new(params.arena_w * 0.75, params.arena_h * 0.5),
        },
    ]
}

/// Build a [`HeadlessRunner`] with one WASM slot ("wasm-team") and one Default
/// slot ("default-team").
fn runner_with_wasm_and_default(max_ticks: u32) -> Arc<HeadlessRunner> {
    let params = fast_params(max_ticks);
    let specs = two_specs(&params);

    let wasm_store = WasmBotStore::new();
    wasm_store.store("wasm-team", wat_to_wasm(CONST_ACTION_WAT));

    HeadlessRunner::new(
        wasm_store,
        RecordingStore::new(),
        params,
        specs,
        vec!["wasm-team".to_owned(), "default-team".to_owned()],
        10_000_000,
        0,
    )
}

/// Build a [`HeadlessRunner`] with two Default slots (no WASM uploaded).
fn runner_all_default(max_ticks: u32) -> Arc<HeadlessRunner> {
    let params = fast_params(max_ticks);
    let specs = two_specs(&params);

    HeadlessRunner::new(
        WasmBotStore::new(),
        RecordingStore::new(),
        params,
        specs,
        vec!["alpha".to_owned(), "beta".to_owned()],
        10_000_000,
        0,
    )
}

// ── Test 1: headless match completes with WASM + Default slots ────────────────
//
// RED → GREEN: HeadlessRunner::run_one_seeded finishes a short match and
// returns a valid HeadlessResult with the expected ticks and scores.
//
// Observable: outcome.ticks == max_ticks, scores has 2 entries, driver_kinds
// contains "wasm" for the WASM slot and "default" for the Default slot.

#[test]
fn headless_match_completes_with_wasm_and_default() {
    let runner = runner_with_wasm_and_default(30);
    let result: HeadlessResult = runner.run_one_seeded(42);

    assert_eq!(result.outcome.ticks, 30, "match must complete all 30 ticks");
    assert_eq!(result.outcome.scores.len(), 2, "one score per ship");
    assert_eq!(result.driver_kinds.len(), 2, "one driver kind per slot");
    assert_eq!(result.driver_kinds[0], "wasm", "slot 0 must be WASM");
    assert_eq!(result.driver_kinds[1], "default", "slot 1 must be Default");
}

// ── Test 2: full-length match completes promptly (no live pacing) ─────────────
//
// RED → GREEN: A 3 600-tick match runs in well under 10 seconds when using
// NoopPacer.  A live-paced match at 30 Hz would take 120 s; proving < 10 s
// shows the runner is truly uncapped.
//
// Observable: outcome.ticks == 3 600, wall-clock elapsed < 10 s.

#[test]
fn headless_match_is_uncapped_completes_full_length_promptly() {
    let runner = runner_all_default(3_600);
    let start = Instant::now();
    let result = runner.run_one_seeded(0);
    let elapsed = start.elapsed();

    assert_eq!(
        result.outcome.ticks, 3_600,
        "full-length match must complete all 3 600 ticks"
    );
    assert!(
        elapsed.as_secs() < 10,
        "headless match must complete in < 10 s (NoopPacer), but took {elapsed:?}"
    );
}

// ── Test 3: WS driver kind is never present in headless results ───────────────
//
// RED → GREEN: The HeadlessRunner always uses an empty WsConnectionRegistry.
// No matter what is in any external registry, the driver_kinds must not
// contain "ws" — only "wasm" or "default".
//
// Observable: driver_kinds contains no "ws" entries across two configurations:
// (a) one WASM + one Default slot, (b) two Default slots (no uploads).

#[test]
fn ws_driver_never_used_in_headless_run() {
    // Configuration A: one WASM, one Default.
    let runner_a = runner_with_wasm_and_default(10);
    let result_a = runner_a.run_one_seeded(1);
    for kind in &result_a.driver_kinds {
        assert_ne!(kind, "ws", "headless run must never use a WS driver");
    }

    // Configuration B: both Default (no WASM uploaded).
    let runner_b = runner_all_default(10);
    let result_b = runner_b.run_one_seeded(2);
    for kind in &result_b.driver_kinds {
        assert_ne!(kind, "ws", "headless run must never use a WS driver");
    }

    // The only valid values are "wasm" or "default".
    let all_kinds: Vec<&str> = result_a
        .driver_kinds
        .iter()
        .chain(result_b.driver_kinds.iter())
        .map(|s| s.as_str())
        .collect();
    for kind in all_kinds {
        assert!(
            kind == "wasm" || kind == "default",
            "headless driver kind must be wasm or default, got: {kind}"
        );
    }
}

// ── Test 4: continuous runner produces N results then stops on signal ─────────
//
// RED → GREEN: spawn_loop produces N HeadlessResults into the channel and
// then halts when the stop sender fires.  The test is bounded: it awaits
// exactly 3 results and then sends the stop signal; the handle must finish
// within a short timeout.
//
// Observable: 3 results arrive on the receiver; the JoinHandle resolves.

#[tokio::test]
async fn continuous_runner_produces_n_results_then_stops_on_signal() {
    let runner = runner_all_default(10);
    let (stop_tx, mut result_rx, handle) = runner.spawn_loop();

    // Collect exactly 3 results.
    let mut results: Vec<HeadlessResult> = Vec::new();
    for _ in 0..3 {
        let r = result_rx.recv().await.expect("channel must not close before 3 results");
        results.push(r);
    }
    assert_eq!(results.len(), 3, "must receive exactly 3 results before stopping");

    // Signal the loop to stop.
    stop_tx.send(true).expect("stop signal send must succeed");

    // The loop must finish; allow generous wall-clock time.
    tokio::time::timeout(std::time::Duration::from_secs(15), handle)
        .await
        .expect("headless loop must finish within 15 s after stop signal")
        .expect("loop task must not panic");
}

// ── Test 5: headless result carries a full ranking ────────────────────────────
//
// RED → GREEN: outcome.scores contains one entry per ship; each score is ≥ 0.0.
// outcome.winner is either Some or None (both are valid; we just assert the
// field is present and scores are non-negative).
//
// Observable: scores.len() == n_ships, all ≥ 0.0.

#[test]
fn headless_result_carries_full_ranking() {
    let params = fast_params(50);
    let specs = two_specs(&params);

    let wasm_store = WasmBotStore::new();
    wasm_store.store("a", wat_to_wasm(CONST_ACTION_WAT));
    wasm_store.store("b", wat_to_wasm(CONST_ACTION_WAT));

    let runner = HeadlessRunner::new(
        wasm_store,
        RecordingStore::new(),
        params.clone(),
        specs,
        vec!["a".to_owned(), "b".to_owned()],
        10_000_000,
        0,
    );

    let result = runner.run_one_seeded(7);

    assert_eq!(
        result.outcome.scores.len(),
        2,
        "scores must have one entry per ship"
    );
    for (ship_id, score) in &result.outcome.scores {
        assert!(
            *score >= 0.0,
            "score for {ship_id} must be non-negative, got {score}"
        );
    }
    // winner field is always present (may be Some or None for a tie).
    let _ = result.outcome.winner; // just confirm it's accessible
}

// ── Test 6: headless match is recorded in RecordingStore ─────────────────────
//
// RED → GREEN: After run_one_seeded, the RecordingStore contains the match.
// The stored recording's seed matches the one passed to run_one_seeded;
// the intent_log is non-empty (at least one tick was recorded).
//
// Observable: store.get(match_id) returns Some(recording) with matching seed
// and intent_log.len() > 0.

#[test]
fn headless_match_is_recorded_to_store() {
    let params = fast_params(20);
    let specs = two_specs(&params);
    let recording_store = RecordingStore::new();

    let runner = HeadlessRunner::new(
        WasmBotStore::new(),
        Arc::clone(&recording_store),
        params,
        specs,
        vec!["p".to_owned(), "q".to_owned()],
        10_000_000,
        0,
    );

    let seed = 99u64;
    let result = runner.run_one_seeded(seed);

    let recording = recording_store
        .get(&result.match_id)
        .expect("recording must be present in store after run_one_seeded");

    assert_eq!(recording.match_id, result.match_id, "match_id must match");
    assert_eq!(recording.seed, seed, "recording seed must match the run seed");
    assert!(
        !recording.intent_log.is_empty(),
        "intent_log must be non-empty (at least one tick recorded)"
    );
    assert_eq!(
        recording.intent_log.len() as u32,
        result.outcome.ticks,
        "intent_log length must equal ticks elapsed"
    );
}
