//! Tests for Recording & Replay (issue 08) and durable persistence (issue B2a).
//!
//! ## TDD tracer order
//!
//! 1. `recording_store_record_and_get`
//!    Finishing a match stores a recording; `store.get(id)` returns
//!    seed + intent_log + params + specs.
//!
//! 2. `recording_store_list_returns_metadata`
//!    `store.list()` returns the recorded match's id + metadata (seed,
//!    tick_count, winner, scores).
//!
//! 3. `replay_match_yields_identical_result`
//!    Replaying a recorded match via `harness::replay_match` gives an
//!    identical winner, scores, and `final_god_view` to the original run.
//!
//! 4. `replay_through_observer_publishes_frames`
//!    `run_replay` publishes one god-view frame per tick through the
//!    `ObserverHub`; a subscriber receives tick-by-tick frames matching the
//!    original match length.
//!
//! 5. `replay_twice_is_deterministic`
//!    Replaying the same recording twice yields identical winners and scores.
//!
//! 6. `recording_store_list_multiple`
//!    Storing two recordings returns both in `list()`.
//!
//! ### B2a: disk persistence
//!
//! 7. `recording_store_with_dir_writes_json_file`
//!    `record()` on a dir-backed store creates a `{match_id}.json` file.
//!
//! 8. `recording_store_with_dir_loads_on_construct`
//!    Constructing a NEW store with the SAME dir sees the previously written recording.
//!
//! 9. `recording_store_restart_round_trip`
//!    Full restart simulation: record → drop store → new store with same dir → get(id).

use arena_engine::harness::{Policy, replay_match, run_match};
use arena_engine::Params;

use arena_server::observer::ObserverHub;
use arena_server::pacer::NoopPacer;
use arena_server::recording::{Recording, RecordingMeta, RecordingStore};
use arena_server::replay::run_replay;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Short match params so tests run fast.
fn short_params() -> Params {
    Params { max_ticks: 30, ..Params::default() }
}

/// Run a quick harness match and package the result into a `Recording`.
fn make_recording(seed: u64) -> Recording {
    let params = short_params();
    let result = run_match(params.clone(), &[Policy::Salvager, Policy::Aggressor], seed);

    let meta = RecordingMeta {
        match_id: "test-match-1".to_owned(),
        seed: result.seed,
        tick_count: result.intent_log.len() as u32,
        winner: result.winner.clone(),
        scores: result.scores.clone(),
    };

    Recording {
        match_id: "test-match-1".to_owned(),
        seed: result.seed,
        params: result.params,
        specs: result.specs,
        intent_log: result.intent_log,
        meta,
    }
}

// ── Test 1: store record + get ────────────────────────────────────────────────

#[test]
fn recording_store_record_and_get() {
    let store = RecordingStore::new();
    let rec = make_recording(1);
    let id = rec.match_id.clone();
    let expected_seed = rec.seed;
    let expected_log_len = rec.intent_log.len();
    let expected_specs_len = rec.specs.len();

    store.record(rec);

    let got = store.get(&id).expect("recording must be present after record()");
    assert_eq!(got.match_id, id);
    assert_eq!(got.seed, expected_seed);
    assert_eq!(got.intent_log.len(), expected_log_len, "intent_log must be stored completely");
    assert_eq!(got.specs.len(), expected_specs_len, "specs must be stored");
    // params round-trip: max_ticks must match
    assert_eq!(got.params.max_ticks, short_params().max_ticks);
}

// ── Test 2: list returns metadata ─────────────────────────────────────────────

#[test]
fn recording_store_list_returns_metadata() {
    let store = RecordingStore::new();
    let rec = make_recording(2);
    let expected_id = rec.match_id.clone();
    let expected_seed = rec.seed;

    store.record(rec);

    let list = store.list();
    assert_eq!(list.len(), 1);
    let meta = &list[0];
    assert_eq!(meta.match_id, expected_id);
    assert_eq!(meta.seed, expected_seed);
    assert!(meta.tick_count > 0, "tick_count should be non-zero");
    // scores should be non-empty (two ships)
    assert_eq!(meta.scores.len(), 2);
}

// ── Test 3: replay gives identical result ─────────────────────────────────────

#[test]
fn replay_match_yields_identical_result() {
    let params = short_params();
    let seed = 99;
    let original = run_match(params.clone(), &[Policy::Salvager, Policy::Aggressor], seed);

    let replayed = replay_match(
        original.params.clone(),
        original.specs.clone(),
        original.seed,
        &original.intent_log,
    );

    // Winner must match.
    assert_eq!(replayed.winner, original.winner, "winner must be identical");

    // Scores must match exactly (engine is deterministic).
    assert_eq!(replayed.scores.len(), original.scores.len());
    for ((orig_id, orig_score), (rep_id, rep_score)) in
        original.scores.iter().zip(replayed.scores.iter())
    {
        assert_eq!(orig_id, rep_id, "ship ids must match");
        assert!(
            (orig_score - rep_score).abs() < f32::EPSILON,
            "score for {orig_id} must be identical: {orig_score} != {rep_score}"
        );
    }

    // Final god-view tick must match.
    assert_eq!(
        replayed.final_god_view.tick,
        original.final_god_view.tick,
        "final tick must match"
    );
    // seed field in god_view
    assert_eq!(replayed.final_god_view.seed, original.final_god_view.seed);
}

// ── Test 4: replay through observer publishes god-view frames ─────────────────

#[test]
fn replay_through_observer_publishes_frames() {
    let hub = ObserverHub::new();
    let mut rx = hub.subscribe();

    let rec = make_recording(7);
    let expected_ticks = rec.intent_log.len();
    assert!(expected_ticks > 0, "need at least one tick to test");

    // Run replay through the hub with a NoopPacer (instant, no sleeping).
    let result = run_replay(&rec, &hub, Box::new(NoopPacer));

    // Every frame published while we had an active subscriber should be
    // received.  Collect them all via try_recv.
    let mut frames_received = 0usize;
    while let Ok(frame) = rx.try_recv() {
        frames_received += 1;
        // Each frame must be valid JSON with type "godView".
        let v: serde_json::Value = serde_json::from_str(&frame)
            .expect("frame must be valid JSON");
        assert_eq!(v["type"], "godView", "frame type must be godView");
    }

    assert_eq!(
        frames_received, expected_ticks,
        "must receive exactly one god-view frame per tick"
    );

    // Replay result parity: winner must match original.
    let original = run_match(short_params(), &[Policy::Salvager, Policy::Aggressor], 7);
    assert_eq!(result.winner, original.winner, "replay result winner must match original");
}

// ── Test 5: replaying twice is deterministic ──────────────────────────────────

#[test]
fn replay_twice_is_deterministic() {
    let rec = make_recording(42);

    let hub = ObserverHub::new();
    let r1 = run_replay(&rec, &hub, Box::new(NoopPacer));
    let r2 = run_replay(&rec, &hub, Box::new(NoopPacer));

    assert_eq!(r1.winner, r2.winner, "winner must be identical across two replays");
    assert_eq!(r1.scores.len(), r2.scores.len());
    for ((id1, s1), (id2, s2)) in r1.scores.iter().zip(r2.scores.iter()) {
        assert_eq!(id1, id2);
        assert!(
            (s1 - s2).abs() < f32::EPSILON,
            "score for {id1} must be identical: {s1} != {s2}"
        );
    }
    assert_eq!(
        r1.final_god_view.tick, r2.final_god_view.tick,
        "final tick must be identical"
    );
}

// ── Test 6: listing multiple recordings returns them all ──────────────────────

#[test]
fn recording_store_list_multiple() {
    let store = RecordingStore::new();

    // Record two distinct matches with distinct IDs.
    let mut rec1 = make_recording(10);
    rec1.match_id = "match-a".to_owned();
    rec1.meta.match_id = "match-a".to_owned();

    let mut rec2 = make_recording(20);
    rec2.match_id = "match-b".to_owned();
    rec2.meta.match_id = "match-b".to_owned();

    store.record(rec1);
    store.record(rec2);

    let list = store.list();
    assert_eq!(list.len(), 2, "both recordings must appear in list()");

    let ids: Vec<&str> = list.iter().map(|m| m.match_id.as_str()).collect();
    assert!(ids.contains(&"match-a"), "match-a must be in list");
    assert!(ids.contains(&"match-b"), "match-b must be in list");
}

// ── Test 7: with_dir writes a JSON file ──────────────────────────────────────

#[test]
fn recording_store_with_dir_writes_json_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = RecordingStore::with_dir(tmp.path().to_path_buf());

    let mut rec = make_recording(77);
    rec.match_id = "persist-match-1".to_owned();
    rec.meta.match_id = "persist-match-1".to_owned();
    store.record(rec);

    let file = tmp.path().join("persist-match-1.json");
    assert!(file.exists(), "recording JSON file must be created on record()");

    let json = std::fs::read_to_string(&file).expect("read file");
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(v["match_id"], "persist-match-1");
    assert!(v["seed"].is_number(), "seed must be in JSON");
    assert!(v["intent_log"].is_array(), "intent_log must be in JSON");
    assert!(v["specs"].is_array(), "specs must be in JSON");
    assert!(v["params"].is_object(), "params must be in JSON");
    assert!(v["meta"].is_object(), "meta must be in JSON");
}

// ── Test 8: with_dir loads existing files on construction ─────────────────────

#[test]
fn recording_store_with_dir_loads_on_construct() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // Write one recording using a first store instance.
    {
        let store = RecordingStore::with_dir(tmp.path().to_path_buf());
        let mut rec = make_recording(88);
        rec.match_id = "pre-existing".to_owned();
        rec.meta.match_id = "pre-existing".to_owned();
        store.record(rec);
    }

    // Construct a SECOND store against the SAME directory.
    let store2 = RecordingStore::with_dir(tmp.path().to_path_buf());
    let got = store2.get("pre-existing");
    assert!(got.is_some(), "second store must find the recording written by the first");
    let got = got.unwrap();
    assert_eq!(got.match_id, "pre-existing");
    assert_eq!(got.params.max_ticks, short_params().max_ticks, "params round-trip");
    assert!(!got.intent_log.is_empty(), "intent_log must round-trip");
    assert!(!got.specs.is_empty(), "specs must round-trip");
}

// ── Test 9: full restart round-trip ──────────────────────────────────────────

#[test]
fn recording_store_restart_round_trip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().to_path_buf();

    let expected_seed;
    let expected_tick_count;

    // Simulate a server session: record a match, then drop the store.
    {
        let store = RecordingStore::with_dir(path.clone());
        let mut rec = make_recording(123);
        rec.match_id = "restart-test".to_owned();
        rec.meta.match_id = "restart-test".to_owned();
        expected_seed = rec.seed;
        expected_tick_count = rec.intent_log.len() as u32;
        store.record(rec);
    }
    // `store` is now dropped (simulated restart).

    // Simulate a fresh server start: construct a new store from the same dir.
    let new_store = RecordingStore::with_dir(path);
    let recovered = new_store
        .get("restart-test")
        .expect("recording must survive simulated restart");

    assert_eq!(recovered.match_id, "restart-test");
    assert_eq!(recovered.seed, expected_seed, "seed must survive restart");
    assert_eq!(
        recovered.intent_log.len() as u32,
        expected_tick_count,
        "intent_log length must survive restart"
    );
    assert_eq!(
        recovered.meta.tick_count, expected_tick_count,
        "meta.tick_count must survive restart"
    );
    assert_eq!(
        recovered.params.max_ticks,
        short_params().max_ticks,
        "params must survive restart"
    );

    // Also verify listing works.
    let list = new_store.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].match_id, "restart-test");
}
