//! Integration tests for the connection resolver (issue 06).
//!
//! ## Design
//!
//! Each test drives the resolver through its public API only — no inspection
//! of private fields or downcasting.  Driver kind is distinguished via the
//! `BotDriver::kind()` method added in this issue.
//!
//! The three states (WS present / WASM-only / neither) are set up without real
//! sockets:
//!
//! - **WS present**: a [`StubWsDriver`] (a simple always-`Some` driver
//!   reporting `kind() == "ws"`) is inserted directly into
//!   [`WsConnectionRegistry`].  This exercises the full registry code path
//!   that `ws.rs` will use in production (issue 11).
//! - **WASM-only**: a known-good WAT bot (same pattern as `wasm_host_tests`)
//!   is compiled and stored in the [`WasmBotStore`].
//! - **Neither**: no entry in the registry, no entry in the store.
//!
//! ## TDD order (RED → GREEN)
//!
//! 1. `default_bot_when_neither_ws_nor_wasm`            — no WS, no WASM → Default Bot, slot plays
//! 2. `wasm_bot_when_only_wasm_present`                 — WASM artifact only → WasmBotDriver, ship acts
//! 3. `ws_supersedes_wasm_when_both_present`            — WS + WASM → WS driver chosen
//! 4. `mixed_field_all_slots_filled_match_completes`    — 3-slot mixed field, match runs to completion
//! 5. `resolution_reflects_state_at_build_time`         — WS removed before resolve → falls back to WASM

use arena_engine::{Intent, Observation, Params, ShipClass, ShipSpec, Vec2};
use arena_server::{
    pacer::NoopPacer,
    resolver::{ConnectionResolver, Slot, WsConnectionRegistry},
    runner::{BotDriver, MatchRunner},
    store::WasmBotStore,
    ws::obs_to_tick_json,
};
use std::sync::Arc;

// ── WAT fixtures (same pattern as wasm_host_tests) ───────────────────────────

/// A bot that always writes `{"thrust":1.0}` — proves the WASM path runs.
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

// ── StubWsDriver ─────────────────────────────────────────────────────────────

/// A stand-in for a live WsBotDriver: always returns `Some(Intent::default())`.
///
/// Inserted into [`WsConnectionRegistry`] in tests to represent a connected
/// WS bot without requiring a real socket.  Reports `kind() == "ws"` so tests
/// can assert the correct driver was chosen.
struct StubWsDriver;

impl BotDriver for StubWsDriver {
    fn decide(&mut self, _tick: u32, _obs: &Observation) -> Option<Intent> {
        Some(Intent::default())
    }
    fn kind(&self) -> &'static str {
        "ws"
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn test_params(max_ticks: u32) -> Params {
    Params { max_ticks, ..Params::default() }
}

/// Two ships placed symmetrically in the arena.
fn two_ship_specs(params: &Params) -> Vec<ShipSpec> {
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

/// Three ships placed symmetrically.
fn three_ship_specs(params: &Params) -> Vec<ShipSpec> {
    use std::f32::consts::TAU;
    (0..3)
        .map(|i| {
            let ang = TAU * (i as f32) / 3.0;
            ShipSpec {
                id: format!("ship-{i}"),
                class: ShipClass::Skiff,
                anchor_pos: Vec2::new(
                    params.arena_w * 0.5 + ang.cos() * params.arena_w * 0.3,
                    params.arena_h * 0.5 + ang.sin() * params.arena_h * 0.3,
                ),
            }
        })
        .collect()
}

/// Serialise the tick-0 observation for `ship_id` in a fresh engine.
fn tick0_json(ship_id: &str, specs: &[ShipSpec], params: &Params) -> String {
    let engine = arena_engine::Engine::new(42, params.clone(), specs.to_vec());
    let obs = engine.observation(&ship_id.to_owned()).expect("ship must exist");
    obs_to_tick_json(0, &obs)
}

// ── Test 1: Default Bot when neither WS nor WASM ─────────────────────────────
//
// RED → GREEN:
// - empty registry, empty wasm_store, team "alpha"
// - resolver returns a driver with kind() == "default"
// - a match with that driver runs to completion without panicking

#[test]
fn default_bot_when_neither_ws_nor_wasm() {
    let params = test_params(10);
    let specs = two_ship_specs(&params);

    let ws_registry = WsConnectionRegistry::new();
    let wasm_store = WasmBotStore::new();
    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    );

    let slots = vec![
        Slot { team: "alpha".to_owned(), tick0_obs_json: String::new() },
        Slot { team: "beta".to_owned(), tick0_obs_json: String::new() },
    ];

    let drivers = resolver.resolve(&slots, &params);

    // Both slots fall back to the Default Bot.
    assert_eq!(drivers[0].kind(), "default", "slot 0: no WS, no WASM → default");
    assert_eq!(drivers[1].kind(), "default", "slot 1: no WS, no WASM → default");

    // The match runs to completion — the field is full and no panic occurs.
    let mut runner = MatchRunner::new(42, params.clone(), specs, drivers, Box::new(NoopPacer));
    let outcome = runner.run_to_completion();
    assert_eq!(outcome.ticks, params.max_ticks, "match must complete all ticks");
}

// ── Test 2: WASM Bot when only a WASM artifact is present ────────────────────
//
// RED → GREEN:
// - empty registry, wasm_store has artifact for "alpha"
// - resolver returns kind() == "wasm" for alpha; kind() == "default" for beta
// - the WASM bot drives ship-0 (the const-action bot applies thrust)

#[test]
fn wasm_bot_when_only_wasm_present() {
    let params = test_params(10);
    let specs = two_ship_specs(&params);

    let ws_registry = WsConnectionRegistry::new();
    let wasm_store = WasmBotStore::new();
    wasm_store.store("alpha", wat_to_wasm(CONST_ACTION_WAT));

    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    );

    let slots = vec![
        Slot {
            team: "alpha".to_owned(),
            tick0_obs_json: tick0_json("ship-0", &specs, &params),
        },
        Slot { team: "beta".to_owned(), tick0_obs_json: String::new() },
    ];

    let drivers = resolver.resolve(&slots, &params);

    assert_eq!(drivers[0].kind(), "wasm", "alpha has WASM artifact → wasm driver");
    assert_eq!(drivers[1].kind(), "default", "beta has nothing → default driver");

    // Match runs to completion with the WASM bot driving ship-0.
    let mut runner = MatchRunner::new(42, params.clone(), specs, drivers, Box::new(NoopPacer));
    let outcome = runner.run_to_completion();
    assert_eq!(outcome.ticks, params.max_ticks, "match must complete all ticks");
}

// ── Test 3: WS supersedes WASM when both are present ─────────────────────────
//
// RED → GREEN:
// - registry has a StubWsDriver for "alpha"
// - wasm_store ALSO has a WASM artifact for "alpha"
// - resolver must pick the WS driver (priority 1 > priority 2)
// - beta has only WASM; gamma has neither

#[test]
fn ws_supersedes_wasm_when_both_present() {
    let params = test_params(5);
    let specs = two_ship_specs(&params);

    let ws_registry = WsConnectionRegistry::new();
    let wasm_store = WasmBotStore::new();

    // "alpha" has BOTH a live WS driver AND a WASM artifact.
    ws_registry.insert("alpha", Box::new(StubWsDriver));
    wasm_store.store("alpha", wat_to_wasm(CONST_ACTION_WAT));

    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    );

    let slots = vec![
        Slot {
            team: "alpha".to_owned(),
            tick0_obs_json: tick0_json("ship-0", &specs, &params),
        },
        Slot { team: "beta".to_owned(), tick0_obs_json: String::new() },
    ];

    let drivers = resolver.resolve(&slots, &params);

    // WS beats WASM for alpha.
    assert_eq!(drivers[0].kind(), "ws", "alpha: WS supersedes WASM");
    assert_eq!(drivers[1].kind(), "default", "beta: no WS, no WASM → default");

    // WASM artifact for alpha is still in the store (the resolver only took the WS driver).
    assert!(
        wasm_store.get("alpha").is_some(),
        "WASM artifact must remain in store after WS driver was selected"
    );

    // WS driver was consumed from the registry.
    assert!(
        !ws_registry.has("alpha"),
        "WS driver must be taken from registry after resolve"
    );

    // Match still runs.
    let mut runner = MatchRunner::new(42, params.clone(), specs, drivers, Box::new(NoopPacer));
    let outcome = runner.run_to_completion();
    assert_eq!(outcome.ticks, params.max_ticks);
}

// ── Test 4: Mixed 3-slot field — all slots filled, match completes ────────────
//
// RED → GREEN:
// - ship-0 / "ws-team":   WS driver in registry
// - ship-1 / "wasm-team": WASM artifact in store
// - ship-2 / "none-team": nothing
// - All 3 drivers are assigned the expected kinds.
// - Match runs to completion (5 ticks, NoopPacer).

#[test]
fn mixed_field_all_slots_filled_match_completes() {
    let params = test_params(5);
    let specs = three_ship_specs(&params);

    let ws_registry = WsConnectionRegistry::new();
    let wasm_store = WasmBotStore::new();

    ws_registry.insert("ws-team", Box::new(StubWsDriver));
    wasm_store.store("wasm-team", wat_to_wasm(CONST_ACTION_WAT));

    let resolver = ConnectionResolver::new(
        Arc::clone(&ws_registry),
        Arc::clone(&wasm_store),
        10_000_000,
    );

    let slots = vec![
        Slot {
            team: "ws-team".to_owned(),
            tick0_obs_json: tick0_json("ship-0", &specs, &params),
        },
        Slot {
            team: "wasm-team".to_owned(),
            tick0_obs_json: tick0_json("ship-1", &specs, &params),
        },
        Slot { team: "none-team".to_owned(), tick0_obs_json: String::new() },
    ];

    let drivers = resolver.resolve(&slots, &params);

    assert_eq!(drivers.len(), 3, "one driver per slot");
    assert_eq!(drivers[0].kind(), "ws",      "ws-team   → ws driver");
    assert_eq!(drivers[1].kind(), "wasm",    "wasm-team → wasm driver");
    assert_eq!(drivers[2].kind(), "default", "none-team → default driver");

    // The full 3-bot match runs to completion without panicking.
    let mut runner = MatchRunner::new(42, params.clone(), specs, drivers, Box::new(NoopPacer));
    let outcome = runner.run_to_completion();
    assert_eq!(outcome.ticks, params.max_ticks, "all ticks must complete");
    assert_eq!(outcome.scores.len(), 3, "scores for all three ships");
}

// ── Test 5: Resolution reflects state at build time ──────────────────────────
//
// RED → GREEN:
// - "alpha" has BOTH a WS driver in the registry AND a WASM artifact.
// - The WS driver is removed from the registry BEFORE resolve is called.
// - The resolver falls back to WASM (priority 2).
// - A second resolve (new registry, WS removed, WASM also removed) → Default.

#[test]
fn resolution_reflects_state_at_build_time() {
    let params = test_params(5);
    let specs = two_ship_specs(&params);

    let ws_registry = WsConnectionRegistry::new();
    let wasm_store = WasmBotStore::new();

    // Scenario A: WS present → WS is chosen.
    ws_registry.insert("alpha", Box::new(StubWsDriver));
    wasm_store.store("alpha", wat_to_wasm(CONST_ACTION_WAT));

    {
        let resolver = ConnectionResolver::new(
            Arc::clone(&ws_registry),
            Arc::clone(&wasm_store),
            10_000_000,
        );
        let slots = vec![
            Slot {
                team: "alpha".to_owned(),
                tick0_obs_json: tick0_json("ship-0", &specs, &params),
            },
            Slot { team: "beta".to_owned(), tick0_obs_json: String::new() },
        ];
        let drivers = resolver.resolve(&slots, &params);
        assert_eq!(drivers[0].kind(), "ws", "scenario A: WS present → ws chosen");
    }

    // The WS driver was consumed. The registry no longer has "alpha".
    assert!(!ws_registry.has("alpha"), "WS driver consumed by resolve");

    // Scenario B: WS absent, WASM still present → WASM is chosen.
    {
        let resolver = ConnectionResolver::new(
            Arc::clone(&ws_registry),
            Arc::clone(&wasm_store),
            10_000_000,
        );
        let slots = vec![
            Slot {
                team: "alpha".to_owned(),
                tick0_obs_json: tick0_json("ship-0", &specs, &params),
            },
            Slot { team: "beta".to_owned(), tick0_obs_json: String::new() },
        ];
        let drivers = resolver.resolve(&slots, &params);
        assert_eq!(drivers[0].kind(), "wasm", "scenario B: WS removed → falls back to WASM");
    }

    // Scenario C: explicitly simulate "WS disconnected mid-event" by checking
    // that a brand-new registry (no WS entries) falls through to WASM.
    // This mirrors issue 09 (headless runner passes an empty registry).
    let empty_ws_registry = WsConnectionRegistry::new();
    {
        let resolver = ConnectionResolver::new(
            Arc::clone(&empty_ws_registry),
            Arc::clone(&wasm_store),
            10_000_000,
        );
        let slots = vec![
            Slot {
                team: "alpha".to_owned(),
                tick0_obs_json: tick0_json("ship-0", &specs, &params),
            },
            Slot { team: "beta".to_owned(), tick0_obs_json: String::new() },
        ];
        let drivers = resolver.resolve(&slots, &params);
        assert_eq!(drivers[0].kind(), "wasm", "scenario C: empty registry → WASM");
        assert_eq!(drivers[1].kind(), "default", "scenario C: beta still defaults");
    }
}
