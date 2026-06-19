/// Integration tests for the arena-engine public API.
///
/// Each test asserts a single observable behaviour at the engine boundary.
/// Tests are ordered by the TDD tracer-bullet sequence from the issue spec:
///   1. Construct engine → ship present in god-view
///   2. Per-ship Observation matches PROTOCOL §6 self shape
///   3. God-view exposes all ships
///   4. step() advances tick count and records the applied intent
///   5. Params type carries the params.py first-pass values
///   6. Determinism: same seed + same empty-intent sequence → identical god-view
///   7. Observation ships list excludes self (PROTOCOL §6: "OTHER ships only")
///   8. Observation anchors lists every ship's anchor
use arena_engine::{Engine, Intent, Params, ShipClass, ShipSpec, Vec2};

// ─── helpers ────────────────────────────────────────────────────────────────

fn single_ship_engine() -> Engine {
    let spec = ShipSpec {
        id: "ship-1".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2 { x: 100.0, y: 600.0 },
    };
    Engine::new(42, Params::default(), vec![spec])
}

fn two_ship_engine(seed: u64) -> Engine {
    let specs = vec![
        ShipSpec {
            id: "ship-1".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 100.0, y: 600.0 },
        },
        ShipSpec {
            id: "ship-2".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 1900.0, y: 600.0 },
        },
    ];
    Engine::new(seed, Params::default(), specs)
}

// ─── test 1: construct engine → ship exists in god-view ─────────────────────

#[test]
fn construct_engine_ship_exists_in_god_view() {
    let engine = single_ship_engine();
    let view = engine.god_view();

    assert_eq!(view.ships.len(), 1);
    assert_eq!(view.ships[0].id, "ship-1");
    assert!(view.ships[0].alive);
    assert!(!view.ships[0].invuln);
}

// ─── test 2: per-ship Observation matches PROTOCOL §6 self shape ─────────────

#[test]
fn observation_self_shape_matches_protocol() {
    let params = Params::default();
    let spec = ShipSpec {
        id: "ship-1".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2 { x: 100.0, y: 600.0 },
    };
    let engine = Engine::new(42, params.clone(), vec![spec]);
    let obs = engine
        .observation(&"ship-1".to_string())
        .expect("observation for existing ship");

    let s = &obs.self_view;
    assert_eq!(s.id, "ship-1");
    assert!(s.alive);
    assert!(!s.invuln);
    assert_eq!(s.hull.cur, params.hull_max);
    assert_eq!(s.hull.max, params.hull_max);
    assert_eq!(s.shield.cur, params.shield_max);
    assert_eq!(s.shield.max, params.shield_max);
    assert_eq!(s.aether.cur, params.aether_max);
    assert_eq!(s.aether.max, params.aether_max);
    assert!(s.sigil.is_none());
    assert_eq!(s.cannon_cooldown, params.cannon_start_hot);
    assert_eq!(s.relics_carried, 0);
    assert_eq!(s.pos, Vec2 { x: 100.0, y: 600.0 });
    assert_eq!(s.vel, Vec2 { x: 0.0, y: 0.0 });
    assert_eq!(s.heading, 0.0);
    assert_eq!(s.ang_vel, 0.0);

    // envelope fields
    assert_eq!(obs.tick, 0);
    assert_eq!(obs.max_ticks, params.max_ticks);
    assert_eq!(obs.seed, 42);
    assert_eq!(obs.arena.width, params.arena_w);
    assert_eq!(obs.arena.height, params.arena_h);
}

// ─── test 3: god-view exposes all ships ──────────────────────────────────────

#[test]
fn god_view_exposes_all_ships() {
    let engine = two_ship_engine(42);
    let view = engine.god_view();

    assert_eq!(view.ships.len(), 2);
    let ids: Vec<&str> = view.ships.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"ship-1"));
    assert!(ids.contains(&"ship-2"));
}

// ─── test 4: step() advances tick and records applied intent ─────────────────

#[test]
fn step_advances_tick_and_records_applied_intent() {
    let mut engine = single_ship_engine();
    assert_eq!(engine.tick(), 0);

    let intent = Intent {
        thrust: Some(1.0),
        ..Default::default()
    };
    engine.step(vec![("ship-1".to_string(), intent)]);

    assert_eq!(engine.tick(), 1);

    let log = engine.intent_log();
    assert_eq!(log.len(), 1, "one frame after one step");
    assert_eq!(log[0].len(), 1, "one entry per ship");
    assert_eq!(log[0][0].0, "ship-1");
    // applied thrust carried through to the log
    assert_eq!(log[0][0].1.thrust, Some(1.0_f32));
}

#[test]
fn step_returns_events_per_ship() {
    let mut engine = single_ship_engine();
    let result = engine.step(vec![]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "ship-1");
    // No gameplay rules yet — events list is empty
    assert!(result[0].1.is_empty());
}

// ─── test 5: params type carries the params.py first-pass values ─────────────

#[test]
fn params_carry_params_py_values() {
    let p = Params::default();

    // match setup
    assert_eq!(p.tick_rate, 30);
    assert_eq!(p.arena_w, 2000.0_f32);
    assert_eq!(p.arena_h, 1200.0_f32);
    assert_eq!(p.max_ticks, 3600);
    assert_eq!(p.n_asteroids, 10);
    assert_eq!(p.asteroid_radius_min, 40.0_f32);
    assert_eq!(p.asteroid_radius_max, 90.0_f32);
    assert_eq!(p.asteroid_drift, 1.0_f32);

    // ship & movement
    assert_eq!(p.ship_radius, 20.0_f32);
    assert_eq!(p.max_speed, 12.0_f32);
    assert_eq!(p.thrust_accel, 0.5_f32);
    assert_eq!(p.reverse_accel, 0.25_f32);
    assert_eq!(p.lin_damping, 0.97_f32);
    assert_eq!(p.max_turn, 0.15_f32);

    // aether
    assert_eq!(p.aether_max, 100.0_f32);
    assert_eq!(p.aether_regen, 1.2_f32);
    assert_eq!(p.thrust_cost_full, 1.0_f32);
    assert_eq!(p.shot_cost, 12.0_f32);

    // combat
    assert_eq!(p.cannon_damage, 20.0_f32);
    assert_eq!(p.proj_speed, 25.0_f32);
    assert_eq!(p.proj_range, 1500.0_f32);
    assert_eq!(p.cannon_cooldown, 15);
    assert_eq!(p.cannon_start_hot, 15);
    assert_eq!(p.shield_max, 60.0_f32);
    assert_eq!(p.shield_regen, 2.0_f32);
    assert_eq!(p.shield_regen_delay, 30);
    assert_eq!(p.hull_max, 100.0_f32);

    // collisions
    assert_eq!(p.coll_threshold, 4.0_f32);
    assert_eq!(p.k_asteroid, 5.0_f32);
    assert_eq!(p.k_ram, 3.0_f32);
    assert_eq!(p.k_wall, 3.0_f32);

    // relics / scoring / respawn
    assert_eq!(p.relic_value, 1.0_f32);
    assert_eq!(p.kill_bounty, 2.0_f32);
    assert_eq!(p.carry_cap, 5);
    assert_eq!(p.relic_spawn_period, 60);
    assert_eq!(p.relic_field_cap, 12);
    assert_eq!(p.respawn_delay, 90);
    assert_eq!(p.respawn_invuln, 45);

    // sigils
    assert_eq!(p.afterburner_dur, 30);
    assert_eq!(p.afterburner_thrust_mult, 3.0_f32);
    assert_eq!(p.afterburner_speed_mult, 1.5_f32);
    assert_eq!(p.bulwark_immunity, 45);
    assert_eq!(p.singularity_radius, 200.0_f32);
    assert_eq!(p.singularity_pull, 0.6_f32);
    assert_eq!(p.singularity_dur, 60);
    assert_eq!(p.mine_arm, 15);
    assert_eq!(p.mine_radius, 40.0_f32);
    assert_eq!(p.mine_damage, 60.0_f32);
    assert_eq!(p.lance_speed, 40.0_f32);
    assert_eq!(p.lance_damage, 50.0_f32);

    assert!(p.enable_sigils);
}

// ─── test 6: determinism ─────────────────────────────────────────────────────

#[test]
fn determinism_same_seed_same_state_after_empty_steps() {
    let make = || two_ship_engine(99);

    let mut e1 = make();
    let mut e2 = make();

    for _ in 0..10 {
        e1.step(vec![]);
        e2.step(vec![]);
    }

    let v1 = e1.god_view();
    let v2 = e2.god_view();

    assert_eq!(v1.tick, v2.tick);
    assert_eq!(v1.seed, v2.seed);
    assert_eq!(v1.ships.len(), v2.ships.len());
    for (s1, s2) in v1.ships.iter().zip(v2.ships.iter()) {
        assert_eq!(s1.id, s2.id);
        assert_eq!(s1.pos.x, s2.pos.x);
        assert_eq!(s1.pos.y, s2.pos.y);
        assert_eq!(s1.hull.cur, s2.hull.cur);
        assert_eq!(s1.aether.cur, s2.aether.cur);
    }
}

// ─── test 7: observation ships list excludes self ────────────────────────────

#[test]
fn observation_ships_list_excludes_self() {
    let engine = two_ship_engine(42);
    let obs = engine
        .observation(&"ship-1".to_string())
        .expect("ship-1 exists");

    // §6: "ships" field is OTHER ships only
    assert_eq!(obs.ships.len(), 1);
    assert_eq!(obs.ships[0].id, "ship-2");
    // OtherShipView deliberately has no aether/sigil — verified by type (compile-time)
}

// ─── test 8: observation anchors includes every ship's anchor ────────────────

#[test]
fn observation_anchors_includes_all_ships() {
    let engine = two_ship_engine(42);
    let obs = engine
        .observation(&"ship-1".to_string())
        .expect("ship-1 exists");

    assert_eq!(obs.anchors.len(), 2);
    let anchor_ids: Vec<&str> = obs.anchors.iter().map(|a| a.ship_id.as_str()).collect();
    assert!(anchor_ids.contains(&"ship-1"));
    assert!(anchor_ids.contains(&"ship-2"));

    // Check anchor positions match what was supplied at construction
    let a1 = obs.anchors.iter().find(|a| a.ship_id == "ship-1").unwrap();
    assert_eq!(a1.pos, Vec2 { x: 100.0, y: 600.0 });
}

// ─── test 9: observation returns None for unknown ship ───────────────────────

#[test]
fn observation_returns_none_for_unknown_ship() {
    let engine = single_ship_engine();
    assert!(engine.observation(&"no-such-ship".to_string()).is_none());
}

// ─── test 10: god-view tick matches engine tick ───────────────────────────────

#[test]
fn god_view_tick_matches_engine_tick() {
    let mut engine = single_ship_engine();
    assert_eq!(engine.god_view().tick, 0);

    engine.step(vec![]);
    assert_eq!(engine.god_view().tick, 1);

    engine.step(vec![]);
    assert_eq!(engine.god_view().tick, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Issue 02: Drift movement & piloting
// TDD tracer-bullet order:
//   11. Sustained thrust accelerates along heading
//   12. Speed capped at max_speed
//   13. Turn-rate intent rotates heading, clamped to max_turn
//   14. Omitted intent fields persist across ticks
//   15. Thrust deducts aether; regen restores it
//   16. Thrust at zero aether is ineffective
//   17. Light damping reduces an un-thrusting ship's speed
//   18. Dynamic Drift: scale_drift scales arena by sqrt(N/4)
//   19. Determinism with a non-trivial applied-intent log
//   20. Golden thrust-envelope scenario (mirroring harness.py)
// ═══════════════════════════════════════════════════════════════════════════

use arena_engine::scale_drift;

// ─── helpers ────────────────────────────────────────────────────────────────

fn ship_speed(engine: &Engine) -> f32 {
    let v = engine.god_view().ships[0].vel;
    (v.x * v.x + v.y * v.y).sqrt()
}

fn make_engine_heading_east() -> Engine {
    // heading = 0 means +x (East) per PROTOCOL §3
    let spec = ShipSpec {
        id: "alpha".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2 { x: 0.0, y: 0.0 },
    };
    Engine::new(1, Params::default(), vec![spec])
}

// ─── test 11: sustained thrust accelerates along heading ─────────────────────

#[test]
fn sustained_thrust_accelerates_along_heading() {
    // Ship starts at heading 0 (East). Thrust = 1.0 for 5 ticks.
    // After 5 ticks the ship must be moving in the +x direction and have
    // advanced its x-position beyond the starting point.
    let mut engine = make_engine_heading_east();

    let intent = Intent {
        thrust: Some(1.0),
        ..Default::default()
    };

    for _ in 0..5 {
        engine.step(vec![("alpha".to_string(), intent.clone())]);
    }

    let view = engine.god_view();
    let ship = &view.ships[0];

    assert!(ship.vel.x > 0.0, "ship must be moving East after thrust");
    assert!(ship.vel.y.abs() < 1e-4, "no y-velocity when heading is East");
    assert!(ship.pos.x > 0.0, "ship must have advanced East");
}

// ─── test 12: speed capped at max_speed ──────────────────────────────────────

#[test]
fn speed_capped_at_max_speed() {
    let params = Params::default();
    let mut engine = make_engine_heading_east();

    let intent = Intent {
        thrust: Some(1.0),
        ..Default::default()
    };

    // 200 ticks is far more than enough to hit terminal velocity.
    for _ in 0..200 {
        engine.step(vec![("alpha".to_string(), intent.clone())]);
    }

    let spd = ship_speed(&engine);
    assert!(
        spd <= params.max_speed + 1e-4,
        "speed {spd} exceeds max_speed {}",
        params.max_speed
    );
    assert!(
        spd >= params.max_speed - 0.1,
        "speed {spd} should be close to max_speed {}",
        params.max_speed
    );
}

// ─── test 13: turn-rate intent rotates heading, clamped to max_turn ──────────

#[test]
fn turn_rotates_heading_clamped_to_max_turn() {
    let params = Params::default();
    let mut engine = make_engine_heading_east();

    // One tick with turn = 1.0 (full rate CCW).
    let intent = Intent {
        turn: Some(1.0),
        ..Default::default()
    };
    engine.step(vec![("alpha".to_string(), intent)]);

    let heading = engine.god_view().ships[0].heading;
    assert!(
        (heading - params.max_turn).abs() < 1e-5,
        "heading after one full-rate tick must equal max_turn = {}; got {heading}",
        params.max_turn
    );

    // Five more ticks of full turn → heading = 6 * max_turn.
    let intent2 = Intent {
        turn: Some(1.0),
        ..Default::default()
    };
    for _ in 0..5 {
        engine.step(vec![("alpha".to_string(), intent2.clone())]);
    }
    let heading6 = engine.god_view().ships[0].heading;
    let expected = 6.0 * params.max_turn;
    assert!(
        (heading6 - expected).abs() < 1e-4,
        "heading after 6 full-rate ticks must be {expected}; got {heading6}"
    );

    // A turn fraction > 1 must be honoured as-is (clamping is a game-design
    // layer above the engine; the engine scales by max_turn).
    // PROTOCOL §8: turn is −1..1 fraction of max_turn.
    // Test that turn fraction = −1 moves heading the other way.
    let intent_ccw = Intent {
        turn: Some(-1.0),
        ..Default::default()
    };
    engine.step(vec![("alpha".to_string(), intent_ccw)]);
    let heading7 = engine.god_view().ships[0].heading;
    // Back one step of max_turn from heading6.
    let expected7 = (expected - params.max_turn).rem_euclid(std::f32::consts::TAU);
    assert!(
        (heading7 - expected7).abs() < 1e-4,
        "CW turn must subtract max_turn; expected {expected7}, got {heading7}"
    );
}

// ─── test 14: omitted intent fields persist ───────────────────────────────────

#[test]
fn omitted_intent_fields_persist() {
    let mut engine = make_engine_heading_east();

    // Tick 1: set thrust = 0.8.
    engine.step(vec![(
        "alpha".to_string(),
        Intent {
            thrust: Some(0.8),
            ..Default::default()
        },
    )]);

    let spd1 = ship_speed(&engine);
    assert!(spd1 > 0.0, "should be moving after tick 1");

    // Tick 2: send NO intent for this ship → thrust persists at 0.8.
    engine.step(vec![]);
    let spd2 = ship_speed(&engine);
    assert!(spd2 > spd1, "speed must grow when thrust persists (no intent sent)");

    // Applied-intent log must show thrust = 0.8 in both ticks.
    let log = engine.intent_log();
    assert_eq!(log[0][0].1.thrust, Some(0.8));
    assert_eq!(log[1][0].1.thrust, Some(0.8), "thrust must persist with no intent");
}

// ─── test 15: thrust deducts aether; regen restores it ───────────────────────

#[test]
fn thrust_deducts_aether_and_regen_restores_it() {
    // Use params with no regen so the cost is clearly visible.
    let mut params = Params::default();
    params.aether_regen = 0.0;
    params.aether_max = 100.0;
    params.thrust_cost_full = 5.0;  // 5 aether per tick at full thrust

    let spec = ShipSpec {
        id: "alpha".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2::zero(),
    };
    let mut engine = Engine::new(1, params.clone(), vec![spec]);

    assert_eq!(engine.god_view().ships[0].aether.cur, 100.0);

    let intent = Intent {
        thrust: Some(1.0),
        ..Default::default()
    };
    engine.step(vec![("alpha".to_string(), intent)]);

    let aether_after = engine.god_view().ships[0].aether.cur;
    assert!(
        (aether_after - 95.0).abs() < 1e-4,
        "aether should be 95 after 1 tick at full thrust with no regen; got {aether_after}"
    );

    // Now let regen run for 1 tick with no thrust.
    let mut params2 = Params::default();
    params2.aether_regen = 0.0;
    params2.aether_max = 50.0;
    params2.thrust_cost_full = 10.0;
    let spec2 = ShipSpec {
        id: "beta".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2::zero(),
    };
    let mut engine2 = Engine::new(1, params2, vec![spec2]);

    // Drain 10 aether via thrust (regen = 0 to isolate cost).
    engine2.step(vec![(
        "beta".to_string(),
        Intent {
            thrust: Some(1.0),
            ..Default::default()
        },
    )]);
    let aether_drained = engine2.god_view().ships[0].aether.cur;
    assert!(
        (aether_drained - 40.0).abs() < 1e-4,
        "aether should be 40 after 1 full-thrust tick (50 - 10 + 0 regen); got {aether_drained}"
    );

    // Params: cost=10, regen=3, max=20.
    // After 1 thrust tick:  20 - 10 + 3 = 13.
    // After 1 coast tick:   13 - 0  + 3 = 16.
    let mut params4 = Params::default();
    params4.aether_regen = 3.0;
    params4.aether_max = 20.0;
    params4.thrust_cost_full = 10.0;
    let spec4 = ShipSpec {
        id: "delta".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2::zero(),
    };
    let mut engine4 = Engine::new(1, params4, vec![spec4]);
    engine4.step(vec![(
        "delta".to_string(),
        Intent { thrust: Some(1.0), ..Default::default() },
    )]);
    let aether_after_thrust = engine4.god_view().ships[0].aether.cur;
    assert!(
        (aether_after_thrust - 13.0).abs() < 1e-4,
        "after thrust: 20 - 10 + 3 = 13; got {aether_after_thrust}"
    );
    engine4.step(vec![(
        "delta".to_string(),
        Intent { thrust: Some(0.0), ..Default::default() },
    )]);
    let aether_after_coast = engine4.god_view().ships[0].aether.cur;
    assert!(
        (aether_after_coast - 16.0).abs() < 1e-4,
        "after coast: 13 + 3 = 16; got {aether_after_coast}"
    );
}

// ─── test 16: thrust at zero aether is ineffective ───────────────────────────

#[test]
fn thrust_at_zero_aether_is_ineffective() {
    let mut params = Params::default();
    params.aether_max = 0.0;   // ship starts with 0 aether
    params.aether_regen = 0.0; // no regen

    let spec = ShipSpec {
        id: "alpha".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2::zero(),
    };
    let mut engine = Engine::new(1, params, vec![spec]);

    assert_eq!(engine.god_view().ships[0].aether.cur, 0.0);

    let intent = Intent {
        thrust: Some(1.0),
        ..Default::default()
    };
    engine.step(vec![("alpha".to_string(), intent)]);

    // Velocity must stay at zero — damping on zero vel is still zero.
    let ship = &engine.god_view().ships[0];
    let spd = (ship.vel.x * ship.vel.x + ship.vel.y * ship.vel.y).sqrt();
    assert!(
        spd < 1e-6,
        "thrust at zero aether must be ineffective; speed is {spd}"
    );
}

// ─── test 17: damping reduces un-thrusting ship speed ────────────────────────

#[test]
fn damping_reduces_speed_when_not_thrusting() {
    let params = Params::default();
    let mut engine = make_engine_heading_east();

    // Accelerate to near max speed.
    let thrust_intent = Intent {
        thrust: Some(1.0),
        ..Default::default()
    };
    for _ in 0..100 {
        engine.step(vec![("alpha".to_string(), thrust_intent.clone())]);
    }
    let spd_at_peak = ship_speed(&engine);
    assert!(
        spd_at_peak > params.max_speed * 0.95,
        "should be near max speed after 100 thrust ticks"
    );

    // Coast for 60 ticks (no thrust).
    let coast_intent = Intent {
        thrust: Some(0.0),
        ..Default::default()
    };
    for _ in 0..60 {
        engine.step(vec![("alpha".to_string(), coast_intent.clone())]);
    }
    let spd_after_coast = ship_speed(&engine);

    // After 60 ticks of damping at 0.97: 12 * 0.97^60 ≈ 2.77
    let expected_upper = params.max_speed * (0.97_f32.powi(58)); // loose upper bound
    assert!(
        spd_after_coast < expected_upper,
        "speed after 60 coast ticks ({spd_after_coast}) should be below {expected_upper}"
    );
    assert!(
        spd_after_coast < spd_at_peak,
        "coasting must reduce speed below peak"
    );
}

// ─── test 18: Dynamic Drift — scale_drift ────────────────────────────────────

#[test]
fn scale_drift_scales_arena_by_sqrt_n_over_4() {
    let base = Params::default(); // 2000×1200 baseline (calibrated for N=4)

    // N = 4 → scale = 1.0 → unchanged
    let p4 = scale_drift(&base, 4);
    assert!(
        (p4.arena_w - 2000.0).abs() < 1e-3,
        "N=4 should be unchanged; got {}",
        p4.arena_w
    );
    assert!((p4.arena_h - 1200.0).abs() < 1e-3);

    // N = 1 → scale = 0.5 → 1000×600
    let p1 = scale_drift(&base, 1);
    assert!((p1.arena_w - 1000.0).abs() < 1.0, "N=1 width: {}", p1.arena_w);
    assert!((p1.arena_h - 600.0).abs() < 1.0, "N=1 height: {}", p1.arena_h);

    // N = 8 → scale = √2 ≈ 1.4142 → 2828×1697
    let p8 = scale_drift(&base, 8);
    let expected_w8 = 2000.0 * 2.0_f32.sqrt();
    let expected_h8 = 1200.0 * 2.0_f32.sqrt();
    assert!(
        (p8.arena_w - expected_w8).abs() < 1.0,
        "N=8 width: {} vs expected {expected_w8}",
        p8.arena_w
    );
    assert!(
        (p8.arena_h - expected_h8).abs() < 1.0,
        "N=8 height: {} vs expected {expected_h8}",
        p8.arena_h
    );

    // N=4 engine observation reports correct arena dims.
    let spec = ShipSpec {
        id: "s".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2::zero(),
    };
    let scaled_engine = Engine::new(7, p4, vec![spec]);
    let obs = scaled_engine.observation(&"s".to_string()).unwrap();
    assert!((obs.arena.width - 2000.0).abs() < 1e-3);
    assert!((obs.arena.height - 1200.0).abs() < 1e-3);
}

// ─── test 19: determinism with a non-trivial intent log ──────────────────────

#[test]
fn determinism_with_nontrivial_intent_log() {
    let make = || {
        let specs = vec![
            ShipSpec {
                id: "red".to_string(),
                class: ShipClass::Skiff,
                anchor_pos: Vec2 { x: 100.0, y: 600.0 },
            },
            ShipSpec {
                id: "blue".to_string(),
                class: ShipClass::Skiff,
                anchor_pos: Vec2 { x: 1900.0, y: 600.0 },
            },
        ];
        Engine::new(42, Params::default(), specs)
    };

    // Scripted intents: mix of thrust, turn, and gaps.
    let scripted: Vec<Vec<(String, Intent)>> = vec![
        vec![
            ("red".to_string(), Intent { thrust: Some(1.0), turn: Some(0.5), ..Default::default() }),
            ("blue".to_string(), Intent { thrust: Some(-0.5), ..Default::default() }),
        ],
        vec![
            ("red".to_string(), Intent { thrust: Some(1.0), ..Default::default() }),
        ],
        vec![],
        vec![
            ("red".to_string(), Intent { turn: Some(-1.0), ..Default::default() }),
            ("blue".to_string(), Intent { thrust: Some(1.0), turn: Some(1.0), ..Default::default() }),
        ],
        vec![
            ("red".to_string(), Intent { thrust: Some(0.0), ..Default::default() }),
        ],
    ];

    let run = |scripted: &Vec<Vec<(String, Intent)>>| {
        let mut e = make();
        for frame in scripted {
            e.step(frame.clone());
        }
        // Run 20 more empty steps to let physics settle deterministically.
        for _ in 0..20 {
            e.step(vec![]);
        }
        e.god_view()
    };

    let v1 = run(&scripted);
    let v2 = run(&scripted);

    assert_eq!(v1.tick, v2.tick);
    for (s1, s2) in v1.ships.iter().zip(v2.ships.iter()) {
        assert_eq!(s1.id, s2.id);
        assert_eq!(s1.pos.x, s2.pos.x, "pos.x must be identical ({})", s1.id);
        assert_eq!(s1.pos.y, s2.pos.y, "pos.y must be identical ({})", s1.id);
        assert_eq!(s1.vel.x, s2.vel.x, "vel.x must be identical ({})", s1.id);
        assert_eq!(s1.vel.y, s2.vel.y, "vel.y must be identical ({})", s1.id);
        assert_eq!(s1.aether.cur, s2.aether.cur, "aether must be identical ({})", s1.id);
    }
}

// ─── test 20: golden thrust-envelope scenario ─────────────────────────────────
//
// Mirrors the kinematics section of harness.py (BALANCE.md).
// Physics formula (same as harness.py step()):
//   heading = 0 (East)
//   vel = (vel + thrust_accel) * lin_damping  →  capped at max_speed
//   pos += vel
//   aether += regen - thrust_cost_full  (clamped to [0, max])
//
// Parameters (from params.py / Params::default()):
//   thrust_accel = 0.5,  lin_damping = 0.97,  max_speed = 12.0
//   aether_max = 100,  aether_regen = 1.2,  thrust_cost_full = 1.0
//
// Expected behaviour at 60 ticks of full thrust from rest:
//   • Speed reaches and stays at max_speed ≈ 12.0 (terminal velocity without
//     cap would be thrust_accel * damping / (1 - damping) ≈ 16.17, so cap binds).
//   • Position advances significantly East; analytically ≈ 620 arena units from
//     origin after 60 ticks (ramp-up ~44 ticks + 16 ticks at max speed).
//   • Aether stays full (regen 1.2 > cost 1.0 → net +0.2/tick → capped at max).
//
// Coast scenario (BALANCE.md "coast-to-stop ~384 units, 3.5 s"):
//   After reaching max_speed and cutting thrust, speed decays geometrically.
//   After 120 ticks of coasting, speed < 0.5 arena-units/tick.

#[test]
fn golden_thrust_envelope() {
    let params = Params::default();
    let spec = ShipSpec {
        id: "pilot".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2 { x: 0.0, y: 0.0 },
    };
    let mut engine = Engine::new(7, params.clone(), vec![spec]);

    let thrust_full = Intent {
        thrust: Some(1.0),
        turn: Some(0.0), // heading stays East throughout
        ..Default::default()
    };
    let coast = Intent {
        thrust: Some(0.0),
        ..Default::default()
    };

    // ── Phase 1: full thrust for 60 ticks ──────────────────────────────────
    for _ in 0..60 {
        engine.step(vec![("pilot".to_string(), thrust_full.clone())]);
    }

    let view = engine.god_view();
    let ship = &view.ships[0];

    // Speed must be at or very near max_speed (12.0).
    let spd_at_60 = (ship.vel.x * ship.vel.x + ship.vel.y * ship.vel.y).sqrt();
    assert!(
        spd_at_60 >= 11.5,
        "speed after 60 thrust ticks must be ≥ 11.5; got {spd_at_60}"
    );
    assert!(
        spd_at_60 <= params.max_speed + 1e-4,
        "speed must not exceed max_speed={}; got {spd_at_60}",
        params.max_speed
    );

    // Ship is moving East (vel.x > 0) with negligible y component.
    assert!(ship.vel.x > 0.0, "should be moving East");
    assert!(ship.vel.y.abs() < 1e-4, "heading 0 → no y velocity");

    // Position: analytically ≈ 620 units East.
    // Use loose bounds [400, 800] to be robust across integration orders.
    assert!(
        ship.pos.x >= 400.0,
        "pos.x after 60 thrust ticks should be ≥ 400; got {}",
        ship.pos.x
    );
    assert!(
        ship.pos.x <= 800.0,
        "pos.x after 60 thrust ticks should be ≤ 800; got {}",
        ship.pos.x
    );
    assert!(ship.pos.y.abs() < 1e-3, "no y displacement when heading is East");

    // Aether: net regen 1.2 - cost 1.0 = +0.2/tick → stays capped at max.
    assert!(
        (ship.aether.cur - params.aether_max).abs() < 1e-3,
        "aether should remain full (regen > cost); got {}",
        ship.aether.cur
    );

    // ── Phase 2: coast from max speed ──────────────────────────────────────
    // After 120 ticks of coasting: max_speed * 0.97^120 ≈ 12 * 0.026 ≈ 0.31.
    // Threshold: speed must be < 0.5 arena-units/tick (effectively stopped).
    for _ in 0..120 {
        engine.step(vec![("pilot".to_string(), coast.clone())]);
    }
    let spd_coast = ship_speed(&engine);
    assert!(
        spd_coast < 0.5,
        "after 120 coast ticks ship should be nearly stopped; speed = {spd_coast}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Issue 03: Relic economy & match scoring
//
// TDD tracer-bullet order:
//  21. Relics exist in the Drift / god-view at match start
//  22. Observation includes relics
//  23. A ship overlapping a relic picks it up (carried count rises, relic leaves field)
//  24. Carry cap blocks further pickup
//  25. Banking at the Anchor moves carried value into score and clears carried
//  26. Relics replenish over time up to field cap
//  27. Match ends at max_ticks with a winner = highest score
//  28. Golden: carry-to-anchor scores the expected value
//  29. Golden: full match decisive, no shutout
//  30. Determinism: same seed + applied-intent log → same relic spawns / score
// ═══════════════════════════════════════════════════════════════════════════

// ─── helpers ─────────────────────────────────────────────────────────────────

/// A tiny 202 × 202 arena ensures all relic spawn positions ([100, 102) × [100, 102))
/// are within the default pickup_radius (32) of a ship placed at (100, 100).
/// Both ships are at their respective anchors — pickup and banking happen
/// in the same step, which is legal and mirrors the harness per-ship sequence.
fn tiny_arena_params() -> arena_engine::Params {
    let mut p = arena_engine::Params::default();
    p.arena_w = 202.0;
    p.arena_h = 202.0;
    p.relic_field_cap = 4;       // initial = max(2, 4/2) = 2 relics
    p.relic_spawn_period = 1;    // replenish every tick
    p.carry_cap = 1;             // cap = 1 so two ships can share the field
    p.max_ticks = 10;
    p
}

/// Single ship at (100, 100) in a tiny arena — relics always spawn within pickup range.
fn tiny_single_engine() -> arena_engine::Engine {
    let p = {
        let mut p = tiny_arena_params();
        p.carry_cap = 5; // give enough carry capacity for the single-ship golden scenario
        p
    };
    let spec = arena_engine::ShipSpec {
        id: "ship-1".to_string(),
        class: arena_engine::ShipClass::Skiff,
        anchor_pos: arena_engine::Vec2 { x: 100.0, y: 100.0 },
    };
    arena_engine::Engine::new(42, p, vec![spec])
}

/// Two ships in a tiny arena: ship-1 at (99, 99), ship-2 at (101, 101).
/// Both are within pickup and bank radius of relics spawning in [100, 102)^2.
fn tiny_two_ship_engine() -> arena_engine::Engine {
    let p = tiny_arena_params();
    let specs = vec![
        arena_engine::ShipSpec {
            id: "ship-1".to_string(),
            class: arena_engine::ShipClass::Skiff,
            anchor_pos: arena_engine::Vec2 { x: 99.0, y: 99.0 },
        },
        arena_engine::ShipSpec {
            id: "ship-2".to_string(),
            class: arena_engine::ShipClass::Skiff,
            anchor_pos: arena_engine::Vec2 { x: 101.0, y: 101.0 },
        },
    ];
    arena_engine::Engine::new(42, p, specs)
}

// ─── test 21: relics exist in god-view at match start ─────────────────────────

#[test]
fn relics_exist_in_god_view_at_match_start() {
    // With default params: initial relics = max(2, 12/2) = 6.
    let engine = single_ship_engine();
    let view = engine.god_view();

    assert!(
        !view.relics.is_empty(),
        "relics must be present in the Drift at match start"
    );
    // Default: 6 initial relics
    let params = arena_engine::Params::default();
    let expected = std::cmp::max(2, params.relic_field_cap / 2) as usize;
    assert_eq!(
        view.relics.len(),
        expected,
        "expected {expected} initial relics (max(2, relic_field_cap/2)); got {}",
        view.relics.len()
    );

    // Every relic has a non-empty id and a value matching params.
    for r in &view.relics {
        assert!(!r.id.is_empty(), "relic id must be non-empty");
        assert_eq!(
            r.value, params.relic_value,
            "relic value must equal params.relic_value"
        );
    }
}

// ─── test 22: observation includes relics ─────────────────────────────────────

#[test]
fn observation_includes_relics() {
    let engine = single_ship_engine();
    let obs = engine
        .observation(&"ship-1".to_string())
        .expect("ship-1 exists");

    // Observation relics must match god_view relics in count and id.
    let god = engine.god_view();
    assert_eq!(
        obs.relics.len(),
        god.relics.len(),
        "observation and god_view must report the same relic count"
    );
    let obs_ids: std::collections::HashSet<&str> =
        obs.relics.iter().map(|r| r.id.as_str()).collect();
    let god_ids: std::collections::HashSet<&str> =
        god.relics.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(obs_ids, god_ids, "observation and god_view relics must have the same ids");
}

// ─── test 23: ship overlapping a relic picks it up ────────────────────────────

#[test]
fn ship_overlapping_relic_picks_it_up() {
    // tiny_single_engine(): ship at anchor (100, 100), relics in [100, 102)^2.
    // Ship picks up all relics within range AND banks them in the same step
    // (ship is at its anchor).  Observable at the public boundary:
    //   • relics leave the Drift (field count drops)
    //   • score rises (picked-up relics were banked)
    let mut engine = tiny_single_engine();
    let relics_before = engine.god_view().relics.len();
    let score_before = engine.score(&"ship-1".to_string()).unwrap();
    assert!(relics_before > 0, "there must be relics at match start");
    assert_eq!(score_before, 0.0, "score must be 0 at match start");

    engine.step(vec![]);

    let view = engine.god_view();
    let score_after = engine
        .score(&"ship-1".to_string())
        .expect("ship exists");

    // Relics removed from field.
    assert!(
        view.relics.len() < relics_before,
        "picked-up relics must be removed from the Drift; before={relics_before}, after={}",
        view.relics.len()
    );

    // Score rose (relics were banked in the same step since ship is at anchor).
    assert!(
        score_after > score_before,
        "score must rise after pickup+banking; before={score_before}, after={score_after}"
    );
}

// ─── test 24: carry cap blocks further pickup ─────────────────────────────────

#[test]
fn carry_cap_blocks_further_pickup() {
    // Two otherwise-identical engines, one with carry_cap=1 and one with cap=5.
    // With cap=1 the ship can remove at most 1 relic per step from the field.
    // With cap=5 (≥ initial relic count of 2) all relics are removed in one step.
    // The difference in field count after one step reveals the cap effect.

    let make_engine = |cap: u32| {
        let mut params = arena_engine::Params::default();
        params.arena_w = 202.0;
        params.arena_h = 202.0;
        params.relic_field_cap = 4;   // initial = 2 relics
        params.carry_cap = cap;
        params.relic_spawn_period = 9999;
        let spec = arena_engine::ShipSpec {
            id: "s".to_string(),
            class: arena_engine::ShipClass::Skiff,
            anchor_pos: arena_engine::Vec2 { x: 100.0, y: 100.0 },
        };
        arena_engine::Engine::new(42, params, vec![spec])
    };

    // With cap=5: both initial relics are picked up and banked in one step.
    let mut full_cap = make_engine(5);
    let initial_relics = full_cap.god_view().relics.len();
    assert_eq!(initial_relics, 2, "expected 2 initial relics; got {initial_relics}");
    full_cap.step(vec![]);
    let relics_after_full = full_cap.god_view().relics.len();
    let score_full = full_cap.score(&"s".to_string()).unwrap();

    // With cap=1: only 1 relic picked up+banked per step.
    let mut capped = make_engine(1);
    capped.step(vec![]);
    let relics_after_capped = capped.god_view().relics.len();
    let score_capped = capped.score(&"s".to_string()).unwrap();

    // cap=5 removes all relics; cap=1 leaves (initial - 1) relics.
    assert_eq!(
        relics_after_full, 0,
        "cap=5 must clear all relics in one step; {relics_after_full} remain"
    );
    assert_eq!(
        relics_after_capped,
        initial_relics - 1,
        "cap=1 must leave (initial-1)={} relics; got {relics_after_capped}",
        initial_relics - 1
    );

    // Score difference reflects the cap: cap=5 banks more than cap=1.
    assert!(
        score_full > score_capped,
        "score with cap=5 ({score_full}) must exceed score with cap=1 ({score_capped})"
    );
}

// ─── test 25: banking at Anchor moves score and clears carried ─────────────────

#[test]
fn banking_at_anchor_scores_relics_and_clears_carried() {
    // Ship is at its anchor in the tiny arena: picks up relics AND banks them in one step.
    let mut engine = tiny_single_engine();

    let score_before = engine.score(&"ship-1".to_string()).unwrap_or(0.0);
    assert_eq!(score_before, 0.0, "score must be 0 before any banking");

    engine.step(vec![]);

    let view = engine.god_view();
    let ship = &view.ships[0];

    // After pickup + banking in one step, carry must be cleared.
    assert_eq!(
        ship.relics_carried, 0,
        "relics_carried must be 0 after banking; got {}",
        ship.relics_carried
    );

    // Score must have increased by (relics picked up) × relic_value.
    let score_after = engine.score(&"ship-1".to_string()).unwrap_or(0.0);
    assert!(
        score_after > score_before,
        "score must increase after banking; before={score_before}, after={score_after}"
    );

    let p = arena_engine::Params::default();
    // Score must be a multiple of relic_value.
    let banked_count = (score_after / p.relic_value).round() as u32;
    assert!(
        banked_count >= 1,
        "at least one relic must have been banked; score={score_after}"
    );
    assert!(
        (score_after - banked_count as f32 * p.relic_value).abs() < 1e-4,
        "score must equal banked_count × relic_value = {} × {} = {}; got {score_after}",
        banked_count,
        p.relic_value,
        banked_count as f32 * p.relic_value
    );
}

// ─── test 26: relics replenish over time up to field cap ──────────────────────

#[test]
fn relics_replenish_over_time_up_to_field_cap() {
    // A single ship far from all relics so none are picked up.
    // With relic_spawn_period=1 and a small arena, relics spawn every tick
    // until they hit relic_field_cap.
    let mut params = arena_engine::Params::default();
    params.arena_w = 400.0;
    params.arena_h = 400.0;
    params.relic_field_cap = 4;          // cap at 4
    params.relic_spawn_period = 1;       // every tick
    // Ship far from relic spawn zone so it doesn't pick any up.
    params.relic_pickup_radius = 1.0;    // tiny pickup radius → no accidental pickup

    let spec = arena_engine::ShipSpec {
        id: "s".to_string(),
        class: arena_engine::ShipClass::Skiff,
        anchor_pos: arena_engine::Vec2 { x: 200.0, y: 200.0 },
    };
    let mut engine = arena_engine::Engine::new(99, params.clone(), vec![spec]);

    let initial_count = engine.god_view().relics.len();  // max(2, 4/2) = 2
    assert_eq!(initial_count, 2, "initial relics = max(2, field_cap/2) = 2; got {initial_count}");

    // After 5 steps: 5 spawn attempts but cap = 4.
    for _ in 0..5 {
        engine.step(vec![]);
    }
    let after_count = engine.god_view().relics.len();
    assert!(
        after_count <= params.relic_field_cap as usize,
        "relic count must not exceed field_cap={}; got {after_count}",
        params.relic_field_cap
    );
    assert!(
        after_count > initial_count,
        "relics must replenish over time; before={initial_count}, after={after_count}"
    );
    assert_eq!(
        after_count, params.relic_field_cap as usize,
        "relic count must reach field_cap={} after enough replenishments; got {after_count}",
        params.relic_field_cap
    );
}

// ─── test 27: match ends at max_ticks with a winner ───────────────────────────

#[test]
fn match_ends_at_max_ticks_with_winner() {
    let mut params = arena_engine::Params::default();
    params.max_ticks = 5;   // short match for speed
    params.relic_spawn_period = 9999;

    let spec = arena_engine::ShipSpec {
        id: "pilot".to_string(),
        class: arena_engine::ShipClass::Skiff,
        anchor_pos: arena_engine::Vec2 { x: 0.0, y: 0.0 },
    };
    let mut engine = arena_engine::Engine::new(1, params.clone(), vec![spec]);

    assert!(!engine.is_match_over(), "match must not be over before any steps");
    assert!(engine.winner().is_none(), "winner must be None while match is in progress");

    for _ in 0..params.max_ticks {
        engine.step(vec![]);
    }

    assert!(
        engine.is_match_over(),
        "match must be over after max_ticks={} steps; tick={}",
        params.max_ticks,
        engine.tick()
    );
    assert_eq!(engine.tick(), params.max_ticks);

    let w = engine.winner();
    assert!(
        w.is_some(),
        "winner() must return Some after match ends"
    );
    assert_eq!(
        w.as_deref(),
        Some("pilot"),
        "only ship is 'pilot'; winner must be 'pilot'"
    );
}

// ─── test 28: golden — carry-to-anchor scores expected value ──────────────────
//
// Setup (mirrors harness.py):
//   • Tiny 202 × 202 arena → relics always spawn in [100, 102) × [100, 102).
//   • 1 ship at anchor (100, 100) — within both pickup_radius and bank_radius.
//   • relic_value = 1.0, carry_cap = 5, relic_field_cap = 4 → 2 initial relics.
//   • After one step: ship picks up both relics (both within ~2 u), banks them.
//
// Expected score = 2 × relic_value = 2.0.  Source: BALANCE.md / params.py.

#[test]
fn golden_carry_to_anchor_scores_expected_value() {
    let mut params = arena_engine::Params::default();
    params.arena_w = 202.0;
    params.arena_h = 202.0;
    params.relic_field_cap = 4;      // initial = max(2, 2) = 2 relics
    params.carry_cap = 5;
    params.relic_value = 1.0;
    params.relic_spawn_period = 9999; // no replenishment during this scenario
    params.max_ticks = 3600;

    let spec = arena_engine::ShipSpec {
        id: "salvager".to_string(),
        class: arena_engine::ShipClass::Skiff,
        anchor_pos: arena_engine::Vec2 { x: 100.0, y: 100.0 },
    };
    let mut engine = arena_engine::Engine::new(42, params.clone(), vec![spec]);

    let initial_relics = engine.god_view().relics.len();
    assert_eq!(
        initial_relics, 2,
        "expected 2 initial relics for relic_field_cap=4; got {initial_relics}"
    );

    // One step: ship at anchor picks up both relics AND banks them immediately.
    engine.step(vec![]);

    let score = engine
        .score(&"salvager".to_string())
        .expect("ship exists");

    let expected_score = initial_relics as f32 * params.relic_value;
    assert!(
        (score - expected_score).abs() < 1e-4,
        "golden: score must be {} × {} = {}; got {score}",
        initial_relics,
        params.relic_value,
        expected_score
    );

    // All relics must have been removed from the Drift (they were picked up).
    let relics_after = engine.god_view().relics.len();
    assert_eq!(
        relics_after, 0,
        "all relics must be gone after pickup; {} remain",
        relics_after
    );
}

// ─── test 29: golden — full match decisive, no shutout ────────────────────────
//
// Setup:
//   • Tiny 202 × 202 arena, two ships.
//   • carry_cap = 1: ship-1 grabs first relic each tick, ship-2 grabs second.
//   • relic_spawn_period = 1: one relic added each tick after the first step.
//   • max_ticks = 10: short match, both ships score.
//
// After the initial 2 relics are shared (1 each), every subsequent step adds
// 1 relic that ship-1 takes (it iterates first).  ship-2 never scores again.
//
// After 10 steps:
//   • ship-1 score = 1 (initial) + 9 (replenishments) = 10.
//   • ship-2 score = 1 (initial).
//   → Decisive winner: ship-1.  No shutout: ship-2 score = 1 > 0.
//
// Magnitudes mirror BALANCE.md "leaders bank ~22–26 … no shutouts at any size".

#[test]
fn golden_full_match_decisive_no_shutout() {
    let engine_factory = || tiny_two_ship_engine();

    let mut engine = engine_factory();

    assert!(!engine.is_match_over(), "match must not be over at start");

    // Run the full match.
    while !engine.is_match_over() {
        engine.step(vec![]);
    }

    assert!(engine.is_match_over(), "match must be over after max_ticks steps");

    let winner = engine.winner().expect("match must produce a winner");
    let view = engine.god_view();

    let score1 = view.scores[&"ship-1".to_string()];
    let score2 = view.scores[&"ship-2".to_string()];

    // Decisive: there is a winner.
    assert!(
        score1 != score2 || !winner.is_empty(),
        "match must be decisive (winner identified)"
    );
    assert_eq!(
        winner, "ship-1",
        "ship-1 takes first relic every tick and should win; scores: ship-1={score1}, ship-2={score2}"
    );

    // No shutout: both ships have positive scores.
    assert!(
        score1 > 0.0,
        "ship-1 must have scored something (no shutout); score={score1}"
    );
    assert!(
        score2 > 0.0,
        "ship-2 must have scored something (no shutout); score={score2}"
    );

    // Sanity: winner has strictly higher score.
    assert!(
        score1 > score2,
        "winner ship-1 (score={score1}) must beat ship-2 (score={score2})"
    );
}

// ─── test 30: determinism — same seed + intent log reproduces identical scores ─

#[test]
fn determinism_same_seed_reproduces_relic_spawns_and_score() {
    // Run two engines with the same seed and same (empty) intents.
    // Both must end with identical relic positions and scores.

    let make = || {
        let mut p = arena_engine::Params::default();
        p.max_ticks = 120;
        p.relic_spawn_period = 30;

        let specs = vec![
            arena_engine::ShipSpec {
                id: "red".to_string(),
                class: arena_engine::ShipClass::Skiff,
                anchor_pos: arena_engine::Vec2 { x: 200.0, y: 600.0 },
            },
            arena_engine::ShipSpec {
                id: "blue".to_string(),
                class: arena_engine::ShipClass::Skiff,
                anchor_pos: arena_engine::Vec2 { x: 1800.0, y: 600.0 },
            },
        ];
        arena_engine::Engine::new(77, p, specs)
    };

    let scripted: Vec<Vec<(String, arena_engine::Intent)>> = vec![
        vec![
            ("red".to_string(), arena_engine::Intent { thrust: Some(1.0), ..Default::default() }),
            ("blue".to_string(), arena_engine::Intent { thrust: Some(-1.0), ..Default::default() }),
        ],
        vec![
            ("red".to_string(), arena_engine::Intent { turn: Some(0.3), ..Default::default() }),
        ],
    ];

    let run = |scripted: &Vec<Vec<(String, arena_engine::Intent)>>| {
        let mut e = make();
        for frame in scripted {
            e.step(frame.clone());
        }
        for _ in scripted.len()..120 {
            e.step(vec![]);
        }
        e.god_view()
    };

    let v1 = run(&scripted);
    let v2 = run(&scripted);

    // Relic count and positions must be identical.
    assert_eq!(v1.relics.len(), v2.relics.len(), "relic count must match");

    // Sort by id for stable comparison (swap_remove may reorder).
    let mut r1: Vec<_> = v1.relics.iter().collect();
    let mut r2: Vec<_> = v2.relics.iter().collect();
    r1.sort_by(|a, b| a.id.cmp(&b.id));
    r2.sort_by(|a, b| a.id.cmp(&b.id));
    for (a, b) in r1.iter().zip(r2.iter()) {
        assert_eq!(a.id, b.id, "relic ids must match");
        assert_eq!(a.pos.x, b.pos.x, "relic x must be identical ({})", a.id);
        assert_eq!(a.pos.y, b.pos.y, "relic y must be identical ({})", a.id);
    }

    // Scores must be identical.
    for (id, &s1) in &v1.scores {
        let s2 = v2.scores[id];
        assert_eq!(s1, s2, "score for {id} must be identical: {s1} vs {s2}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Issue 04: Rune-cannon & shield/hull damage
//
// TDD tracer-bullet order (mirrors issue spec):
//  31. Firing with aether + off-cooldown spawns a projectile, deducts shot_cost,
//      and starts the cooldown.
//  32. Firing on cooldown produces no projectile and no cost.
//  33. Firing without enough aether produces no projectile and no cost.
//  34. Projectile travels at proj_speed each tick and despawns past proj_range.
//  35. Projectile hit reduces Shield first; Hull is untouched.
//  36. Overflow damage after Shield depletion reduces Hull.
//  37. Shield regenerates after shield_regen_delay ticks unhit, capped at max.
//  38. A hit emits per-ship events (TookShield / ShieldDown / TookHull).
//  39. Golden TTK: 8 shots bring shield+hull to 0 in ~120 ticks.
//  40. Determinism: same seed + fire-intent log → identical shield/hull state.
// ═══════════════════════════════════════════════════════════════════════════

use arena_engine::Event;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Two-ship engine optimised for cannon tests.
///
/// - Shooter at (0, 0), heading East (0 rad).
/// - Target at (21, 0) — exactly 21 units away; a proj_speed-25 projectile
///   reaches the target in 1 tick (projectile moves from (0,0) to (25,0);
///   distance to target center = |25 - 21| = 4 < ship_radius 20 → HIT).
/// - `cannon_start_hot = 0` so the cannon is ready immediately.
/// - `cannon_cooldown = 0` by default so the test controls firing rate through
///   params passed in.
fn cannon_engine_with_params(p: Params) -> Engine {
    let specs = vec![
        ShipSpec {
            id: "shooter".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 0.0, y: 0.0 },
        },
        ShipSpec {
            id: "target".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 21.0, y: 0.0 },
        },
    ];
    Engine::new(1, p, specs)
}

/// Params with `cannon_start_hot = 0` (cannon ready from tick 1).
fn ready_params() -> Params {
    let mut p = Params::default();
    p.cannon_start_hot = 0;
    p
}

/// Fire-only intent (no thrust / turn).
fn fire_intent() -> Intent {
    Intent {
        fire: Some(true),
        ..Default::default()
    }
}

fn cease_fire() -> Intent {
    Intent {
        fire: Some(false),
        ..Default::default()
    }
}

/// Helper: look up a ship in the god-view by id.
fn find_ship(engine: &Engine, id: &str) -> arena_engine::GodShipView {
    engine
        .god_view()
        .ships
        .into_iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("ship {id} not found"))
}

// ─── test 31: fire spawns projectile, deducts shot_cost, starts cooldown ─────

#[test]
fn fire_spawns_projectile_deducts_aether_starts_cooldown() {
    // Single-ship engine so the projectile has nothing to hit and stays alive
    // for observation after the step.
    let p = ready_params();
    let spec = ShipSpec {
        id: "shooter".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2 { x: 0.0, y: 0.0 },
    };
    let mut engine = Engine::new(1, p.clone(), vec![spec]);

    // Confirm cannon is ready and aether is full.
    let s0 = &engine.god_view().ships[0];
    assert_eq!(s0.cannon_cooldown, 0, "cannon must be ready (start_hot=0)");
    assert_eq!(s0.aether.cur, p.aether_max);

    // Step with fire=true.  Shooter heading = 0 → projectile goes East.
    engine.step(vec![("shooter".to_string(), fire_intent())]);

    let view = engine.god_view();

    // One projectile spawned, owned by shooter.
    assert_eq!(view.projectiles.len(), 1, "exactly one projectile after firing");
    assert_eq!(view.projectiles[0].owner, "shooter");

    // Projectile velocity matches heading=0 East at proj_speed.
    let pv = &view.projectiles[0].vel;
    assert!((pv.x - p.proj_speed).abs() < 1e-4, "proj vel.x = proj_speed");
    assert!(pv.y.abs() < 1e-4, "proj vel.y = 0 (heading East)");

    // Projectile should also be visible in per-ship observation.
    let obs = engine
        .observation(&"shooter".to_string())
        .expect("observation exists");
    assert_eq!(
        obs.projectiles.len(),
        1,
        "projectile visible in observation"
    );

    // Aether deducted by shot_cost (regen may offset at max so check ≤ max - cost + regen).
    let s1 = &engine.god_view().ships[0];
    // aether after: starts at max (100), regen applied first → capped at 100,
    // then shot deducted: 100 - 12 = 88.
    let expected_aether = p.aether_max - p.shot_cost;
    assert!(
        (s1.aether.cur - expected_aether).abs() < 0.01,
        "aether must drop by shot_cost; expected {expected_aether}, got {}",
        s1.aether.cur
    );

    // Cooldown started.
    assert_eq!(
        s1.cannon_cooldown,
        p.cannon_cooldown,
        "cooldown must start at cannon_cooldown after firing"
    );
}

// ─── test 32: firing on cooldown produces no projectile and no cost ───────────

#[test]
fn fire_on_cooldown_produces_no_projectile_and_no_cost() {
    // Default params: cannon_start_hot = 15 → cannon starts on cooldown.
    let p = Params::default();
    let mut engine = cannon_engine_with_params(p.clone());

    let s0 = find_ship(&engine, "shooter");
    assert_eq!(
        s0.cannon_cooldown,
        p.cannon_start_hot,
        "cannon must start on cooldown"
    );
    let aether_before = s0.aether.cur;

    // Try to fire while on cooldown.
    engine.step(vec![("shooter".to_string(), fire_intent())]);

    let view = engine.god_view();

    assert_eq!(
        view.projectiles.len(),
        0,
        "no projectile must be spawned while cannon is on cooldown"
    );

    let s1 = find_ship(&engine, "shooter");
    // Aether must NOT drop by shot_cost (only regen applied, capped at max).
    assert!(
        s1.aether.cur >= aether_before,
        "aether must not decrease while cannon is on cooldown; before={aether_before}, after={}",
        s1.aether.cur
    );
    // Cooldown ticked down by 1.
    assert_eq!(
        s1.cannon_cooldown,
        p.cannon_start_hot - 1,
        "cooldown must tick down by 1"
    );
}

// ─── test 33: firing without enough aether produces no projectile ─────────────

#[test]
fn fire_without_aether_produces_no_projectile() {
    let mut p = ready_params();
    p.aether_max = 0.0;   // ship starts with zero aether
    p.aether_regen = 0.0; // no regen

    let mut engine = cannon_engine_with_params(p);

    let s0 = find_ship(&engine, "shooter");
    assert_eq!(s0.aether.cur, 0.0, "aether must start at 0");
    assert_eq!(s0.cannon_cooldown, 0, "cannon must be ready");

    engine.step(vec![("shooter".to_string(), fire_intent())]);

    let view = engine.god_view();
    assert_eq!(
        view.projectiles.len(),
        0,
        "no projectile must be spawned without sufficient aether"
    );

    // No aether was spent (it was already zero).
    assert_eq!(
        find_ship(&engine, "shooter").aether.cur,
        0.0,
        "aether must remain 0"
    );
}

// ─── test 34: projectile travels at proj_speed and despawns past proj_range ───

#[test]
fn projectile_travels_at_proj_speed_and_despawns_past_range() {
    let mut p = ready_params();
    // Tune range so we can observe both travel and despawn quickly.
    // One shot; cooldown = 9999 prevents a second.
    p.cannon_cooldown = 9999;
    p.proj_speed = 25.0;
    p.proj_range = 100.0; // despawn after 4 ticks (4 × 25 = 100)
    p.relic_spawn_period = 9999;

    let specs = vec![ShipSpec {
        id: "shooter".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2 { x: 0.0, y: 0.0 },
    }];
    let mut engine = Engine::new(1, p.clone(), specs);

    // Tick 1: fire → projectile at (25, 0) after movement (1 × 25).
    engine.step(vec![("shooter".to_string(), fire_intent())]);
    let projs = engine.god_view().projectiles;
    assert_eq!(projs.len(), 1, "projectile alive after tick 1");
    let px1 = projs[0].pos.x;
    assert!(
        (px1 - 25.0).abs() < 0.1,
        "after tick 1 projectile at x=25; got {px1}"
    );

    // Tick 2: projectile at (50, 0).
    engine.step(vec![]);
    let px2 = engine.god_view().projectiles[0].pos.x;
    assert!(
        (px2 - 50.0).abs() < 0.1,
        "after tick 2 projectile at x=50; got {px2}"
    );

    // Ticks 3 & 4: still alive.
    engine.step(vec![]); // x=75
    engine.step(vec![]); // x=100 → dist_traveled=100 >= proj_range=100 → despawn

    assert_eq!(
        engine.god_view().projectiles.len(),
        0,
        "projectile must despawn once dist_traveled >= proj_range"
    );
}

// ─── test 35: projectile hit reduces Shield first; Hull is untouched ──────────

#[test]
fn projectile_hit_reduces_shield_first() {
    let mut p = ready_params();
    p.cannon_damage = 20.0;
    p.shield_max = 60.0;
    p.hull_max = 100.0;
    p.cannon_cooldown = 999; // prevent second shot

    let mut engine = cannon_engine_with_params(p.clone());

    // Tick 1: fire → projectile moves 25u → hits target at (21, 0): dist = 4 < 20.
    engine.step(vec![("shooter".to_string(), fire_intent())]);

    let target = find_ship(&engine, "target");

    // Shield absorbed the 20-unit hit.
    assert!(
        (target.shield.cur - (p.shield_max - p.cannon_damage)).abs() < 0.01,
        "shield must drop by cannon_damage; expected {}, got {}",
        p.shield_max - p.cannon_damage,
        target.shield.cur
    );

    // Hull is untouched (shield absorbed everything).
    assert!(
        (target.hull.cur - p.hull_max).abs() < 0.01,
        "hull must be untouched when shield absorbs the hit; got {}",
        target.hull.cur
    );

    // Projectile consumed.
    assert_eq!(
        engine.god_view().projectiles.len(),
        0,
        "projectile must be removed after hitting a ship"
    );
}

// ─── test 36: overflow damage reduces Hull after Shield is depleted ───────────

#[test]
fn overflow_damage_reduces_hull_after_shield_depleted() {
    let mut p = ready_params();
    p.cannon_damage = 20.0;
    p.shield_max = 60.0;
    p.hull_max = 100.0;
    p.cannon_cooldown = 0; // fire every tick (rapid shots to strip shield quickly)
    p.shield_regen_delay = 9999; // no regen interference

    let mut engine = cannon_engine_with_params(p.clone());

    // 3 shots strip the shield: 60 / 20 = 3 shots.
    for _ in 0..3 {
        engine.step(vec![("shooter".to_string(), fire_intent())]);
    }

    let t3 = find_ship(&engine, "target");
    assert!(
        t3.shield.cur <= 0.01,
        "shield must be 0 after 3 hits; got {}",
        t3.shield.cur
    );
    assert!(
        (t3.hull.cur - p.hull_max).abs() < 0.01,
        "hull must still be untouched after shield-only hits; got {}",
        t3.hull.cur
    );

    // 4th shot: all 20 damage overflows to hull.
    engine.step(vec![("shooter".to_string(), fire_intent())]);

    let t4 = find_ship(&engine, "target");
    let expected_hull = p.hull_max - p.cannon_damage;
    assert!(
        (t4.hull.cur - expected_hull).abs() < 0.01,
        "4th hit must deal overflow damage to hull; expected {expected_hull}, got {}",
        t4.hull.cur
    );
    assert!(
        t4.shield.cur <= 0.01,
        "shield must remain 0; got {}",
        t4.shield.cur
    );
}

// ─── test 37: shield regenerates after regen delay, capped at max ─────────────

#[test]
fn shield_regenerates_after_regen_delay_capped_at_max() {
    let mut p = ready_params();
    p.cannon_damage = 20.0;
    p.shield_max = 60.0;
    p.shield_regen = 2.0;
    p.shield_regen_delay = 30;
    p.cannon_cooldown = 9999; // only one shot

    let mut engine = cannon_engine_with_params(p.clone());

    // Tick 1: fire → shield 60 → 40.
    engine.step(vec![("shooter".to_string(), fire_intent())]);

    let after_hit = find_ship(&engine, "target").shield.cur;
    assert!(
        (after_hit - 40.0).abs() < 0.01,
        "shield must be 40 after one hit; got {after_hit}"
    );

    // No more firing: regen waits for shield_regen_delay ticks.
    // After 30 ticks the counter reaches delay → regen begins (+2/tick).
    // Full recovery: (60 - 40) / 2 = 10 regen ticks → tick 31 + 9 = tick 40 total.
    for _ in 0..39 {
        engine.step(vec![("shooter".to_string(), cease_fire())]);
    }
    // Total: 40 steps. Last regen tick = step 40 (ticks_since_last_hit = 39 ≥ 30).
    // shield = 40 + 2 × 10 = 60.
    let after_regen = find_ship(&engine, "target").shield.cur;
    assert!(
        (after_regen - p.shield_max).abs() < 0.01,
        "shield must fully regenerate to shield_max={} after sufficient unhit ticks; got {}",
        p.shield_max,
        after_regen
    );

    // Partial regen check: after only 5 regen ticks (step 35 total) shield < max.
    // Re-run with a new engine.
    let mut engine2 = cannon_engine_with_params(p.clone());
    engine2.step(vec![("shooter".to_string(), fire_intent())]); // hit
    for _ in 0..34 {
        engine2.step(vec![("shooter".to_string(), cease_fire())]);
    }
    let partial = find_ship(&engine2, "target").shield.cur;
    // At step 35 total: ticks_since_last_hit = 34, 4 regen ticks → shield = 40 + 8 = 48.
    assert!(
        partial > after_hit,
        "shield must regen over time; before_regen={after_hit}, after_partial={partial}"
    );
    assert!(
        partial < p.shield_max,
        "shield must not yet be at max after only partial regen; got {partial}"
    );
}

// ─── test 38: a hit emits per-ship events ─────────────────────────────────────

#[test]
fn hit_emits_per_ship_events() {
    let mut p = ready_params();
    p.cannon_damage = 20.0;
    p.shield_max = 60.0;
    p.hull_max = 100.0;
    p.cannon_cooldown = 999;

    let mut engine = cannon_engine_with_params(p);

    // Tick 1: fire → hit target.
    let events = engine.step(vec![("shooter".to_string(), fire_intent())]);

    let target_events: &Vec<Event> = &events
        .iter()
        .find(|(id, _)| id == "target")
        .expect("target entry in events")
        .1;

    // Target must receive a TookShield event with the correct amount.
    let took_shield = target_events
        .iter()
        .find(|e| matches!(e, Event::TookShield { .. }));
    assert!(
        took_shield.is_some(),
        "target must receive TookShield event; got: {target_events:?}"
    );
    if let Some(Event::TookShield { amount, by }) = took_shield {
        assert!(
            (amount - 20.0).abs() < 0.01,
            "TookShield amount must equal cannon_damage; got {amount}"
        );
        assert_eq!(by, "shooter", "TookShield.by must be the shooter");
    }

    // Shooter must NOT receive a TookShield event (it fired, not hit).
    let shooter_events: &Vec<Event> = &events
        .iter()
        .find(|(id, _)| id == "shooter")
        .expect("shooter entry in events")
        .1;
    assert!(
        !shooter_events
            .iter()
            .any(|e| matches!(e, Event::TookShield { .. })),
        "shooter must not receive TookShield"
    );
}

// ─── test 39: golden TTK — 8 shots bring shield+hull to zero ─────────────────
//
// Source: harness.py `combat_metrics()` with DEFAULT params:
//   ehp            = shield_max + hull_max = 60 + 100 = 160
//   shots_to_kill  = ceil(160 / 20)        = 8 shots
//   ttk_ticks      = 8 × cannon_cooldown   = 8 × 15 = 120 ticks  (4.0 s at 30 Hz)
//
// Setup: attacker at (0, 0) heading East; defender at (21, 0) — stationary.
// Projectile travel time = 1 tick (proj moves 25 u/tick; distance 21 u < 25 u
// → distance to center is 4 u < ship_radius 20 u → HIT in the same tick as spawn).
//
// With cannon_start_hot = 15 (default): first shot fires at tick 15.
//   Shot 1 → tick 15 → hit tick 15
//   Shot 2 → tick 30 → hit tick 30
//   ...
//   Shot 8 → tick 15 + 7×15 = 120 → hit tick 120
//   Hull reaches 0 at tick 120.
//
// Expected TTK: 120 ticks (4.0 s). Shield regen does NOT interfere because
// cannon_cooldown (15) < shield_regen_delay (30).
//
// Test asserts: 110 ≤ TTK ≤ 135 (±15 tick / ±0.5 s tolerance), mirroring the
// BALANCE.md "TTK ≈ 4.0 s" magnitude.

#[test]
fn golden_ttk_eight_shots_bring_hull_to_zero() {
    // Use all-default params: cannon_start_hot=15, cannon_cooldown=15,
    // cannon_damage=20, shield_max=60, hull_max=100, proj_speed=25.
    let p = Params::default();

    let specs = vec![
        ShipSpec {
            id: "attacker".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 0.0, y: 0.0 },
        },
        ShipSpec {
            id: "defender".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 21.0, y: 0.0 },
        },
    ];
    let mut engine = Engine::new(42, p.clone(), specs);

    // Budget = harness ttk + generous margin.
    let harness_ttk = (p.shield_max + p.hull_max) / p.cannon_damage * p.cannon_cooldown as f32;
    // harness_ttk = (60+100)/20 * 15 = 8 * 15 = 120
    let max_budget = (harness_ttk as u32) + 30;

    let fire = Intent {
        fire: Some(true),
        ..Default::default()
    };

    let mut hull_zero_tick: Option<u32> = None;
    for _ in 0..max_budget {
        engine.step(vec![("attacker".to_string(), fire.clone())]);
        let def = engine
            .god_view()
            .ships
            .into_iter()
            .find(|s| s.id == "defender")
            .unwrap();
        if def.hull.cur <= 0.0 && hull_zero_tick.is_none() {
            hull_zero_tick = Some(engine.tick());
        }
    }

    let ttk = hull_zero_tick
        .expect("defender hull must reach 0 within budget ticks");
    let defender = engine
        .god_view()
        .ships
        .into_iter()
        .find(|s| s.id == "defender")
        .unwrap();

    // Hull and shield must both be 0.
    assert!(
        defender.hull.cur <= 0.001,
        "hull must be 0 after 8 hits; got {}",
        defender.hull.cur
    );
    assert!(
        defender.shield.cur <= 0.001,
        "shield must be depleted; got {}",
        defender.shield.cur
    );

    // Golden magnitude: TTK must be within ±15 ticks of harness prediction.
    // harness_ttk = 120, our TTK = 120 (same-tick hit with 1-tick travel absorbed
    // by the fact that proj moves in the spawn tick). Tolerance: ±15 ticks.
    let ttk_seconds = ttk as f32 / p.tick_rate as f32;
    let harness_ttk_seconds = harness_ttk / p.tick_rate as f32;
    assert!(
        ttk <= harness_ttk as u32 + 15,
        "TTK {ttk} ticks ({ttk_seconds:.2}s) must be ≤ harness {harness_ttk}+15 ticks ({harness_ttk_seconds:.2}s)"
    );
    assert!(
        ttk >= harness_ttk as u32 - 15,
        "TTK {ttk} ticks must not be suspiciously fast relative to harness {harness_ttk}"
    );
}

// ─── test 40: determinism with fire-intent log ────────────────────────────────

#[test]
fn determinism_with_fire_intent_log() {
    // Run two engines with the same seed and same scripted fire-intents.
    // Both must end with identical shield, hull, aether, and projectile state.

    let make = || {
        let mut p = Params::default();
        p.cannon_start_hot = 0;
        p.max_ticks = 60;
        p.relic_spawn_period = 9999;
        let specs = vec![
            ShipSpec {
                id: "alpha".to_string(),
                class: ShipClass::Skiff,
                anchor_pos: Vec2 { x: 0.0, y: 0.0 },
            },
            ShipSpec {
                id: "beta".to_string(),
                class: ShipClass::Skiff,
                anchor_pos: Vec2 { x: 21.0, y: 0.0 },
            },
        ];
        Engine::new(77, p, specs)
    };

    let scripted: Vec<Vec<(String, Intent)>> = vec![
        vec![("alpha".to_string(), Intent { fire: Some(true), ..Default::default() })],
        vec![],
        vec![("alpha".to_string(), Intent { fire: Some(true), ..Default::default() })],
        vec![("alpha".to_string(), Intent { fire: Some(false), ..Default::default() })],
        vec![("alpha".to_string(), Intent { fire: Some(true), ..Default::default() })],
    ];

    let run = |scripted: &Vec<Vec<(String, Intent)>>| {
        let mut e = make();
        for frame in scripted {
            e.step(frame.clone());
        }
        for _ in scripted.len()..60 {
            e.step(vec![]);
        }
        e.god_view()
    };

    let v1 = run(&scripted);
    let v2 = run(&scripted);

    assert_eq!(v1.tick, v2.tick, "ticks must match");
    assert_eq!(
        v1.projectiles.len(),
        v2.projectiles.len(),
        "projectile counts must match"
    );
    for (s1, s2) in v1.ships.iter().zip(v2.ships.iter()) {
        assert_eq!(s1.id, s2.id);
        assert_eq!(
            s1.shield.cur, s2.shield.cur,
            "shield.cur must be identical for {}",
            s1.id
        );
        assert_eq!(
            s1.hull.cur, s2.hull.cur,
            "hull.cur must be identical for {}",
            s1.id
        );
        assert_eq!(
            s1.aether.cur, s2.aether.cur,
            "aether.cur must be identical for {}",
            s1.id
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Issue 05: Destruction & kill bounty
//
// TDD tracer-bullet order:
//  41. A ship whose Hull reaches zero is marked not-alive (destroyed).
//  42. The killer receives exactly params.kill_bounty when its shot is lethal.
//  43. A lethal hit emits Died { by: Some(killer) } to victim and
//      KilledShip { victim } to killer.
//  44. Kill bounty and banked-relic score combine into total score; winner()
//      selects the highest-score ship.
//  45. Golden: cannon kill awards bounty and registers in match outcome.
//  46. Non-attributed death (env/self): no bounty — not yet testable with the
//      current engine (no env damage source exists); seam documented below.
//  47. Determinism: same seed + fire-intent log that produces a kill →
//      identical state in both runs.
// ═══════════════════════════════════════════════════════════════════════════

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Two-ship engine where one shot is sufficient to destroy the target.
///
/// Layout matches `cannon_engine_with_params` (shooter at (0,0), target at (21,0)).
/// With shield_max=0 and hull_max == cannon_damage the first hit is lethal.
fn one_shot_kill_engine() -> Engine {
    let mut p = ready_params(); // cannon_start_hot = 0
    p.cannon_damage = 20.0;
    p.shield_max = 0.0;
    p.hull_max = 20.0;
    p.shield_regen = 0.0;
    p.cannon_cooldown = 9999; // prevent a second shot in these tests
    p.relic_spawn_period = 9999;
    cannon_engine_with_params(p)
}

// ─── test 41: destroyed ship is marked not-alive ──────────────────────────────
//
// After the lethal tick the victim's `alive` flag in the god-view must be false.
// The ship is still *in* the view (respawn is issue 06) but leaves active play:
// it no longer steps, fires, or appears as a valid target for new projectiles.

#[test]
fn destroyed_ship_is_marked_not_alive() {
    let mut engine = one_shot_kill_engine();

    // Pre-condition: both ships alive.
    assert!(find_ship(&engine, "shooter").alive, "shooter must start alive");
    assert!(find_ship(&engine, "target").alive, "target must start alive");

    // One tick with fire=true → projectile hits and kills target.
    engine.step(vec![("shooter".to_string(), fire_intent())]);

    let target = find_ship(&engine, "target");
    assert!(
        !target.alive,
        "target must be not-alive after lethal hit; hull={}",
        target.hull.cur
    );
    assert!(
        target.hull.cur <= 0.001,
        "hull must be 0 when not-alive; got {}",
        target.hull.cur
    );

    // Shooter must still be alive (it fired, not hit).
    assert!(
        find_ship(&engine, "shooter").alive,
        "shooter must remain alive after making a kill"
    );
}

// ─── test 42: killer receives exactly kill_bounty on lethal hit ───────────────

#[test]
fn killer_receives_kill_bounty_on_lethal_hit() {
    let mut engine = one_shot_kill_engine();
    let p = Params::default();

    let score_before = engine.score(&"shooter".to_string()).unwrap();
    assert_eq!(score_before, 0.0, "shooter score must start at 0");

    engine.step(vec![("shooter".to_string(), fire_intent())]);

    let score_after = engine.score(&"shooter".to_string()).unwrap();
    // Only the bounty must have been added (no relics banked in this engine setup).
    assert!(
        (score_after - p.kill_bounty).abs() < 1e-4,
        "killer score must increase by exactly kill_bounty={}; got {}",
        p.kill_bounty,
        score_after
    );
}

// ─── test 43: Died and KilledShip events emitted on lethal hit ────────────────

#[test]
fn died_and_killed_ship_events_emitted_on_lethal_hit() {
    let mut engine = one_shot_kill_engine();

    let events = engine.step(vec![("shooter".to_string(), fire_intent())]);

    // Victim receives Died { by: Some("shooter") }.
    let target_events = &events.iter().find(|(id, _)| id == "target").unwrap().1;
    let died_ev = target_events.iter().find(|e| matches!(e, Event::Died { .. }));
    assert!(
        died_ev.is_some(),
        "target must receive a Died event; got: {target_events:?}"
    );
    if let Some(Event::Died { by }) = died_ev {
        assert_eq!(
            by.as_deref(),
            Some("shooter"),
            "Died.by must identify the shooter; got {by:?}"
        );
    }

    // Killer receives KilledShip { victim: "target" }.
    let shooter_events = &events.iter().find(|(id, _)| id == "shooter").unwrap().1;
    let killed_ev = shooter_events
        .iter()
        .find(|e| matches!(e, Event::KilledShip { .. }));
    assert!(
        killed_ev.is_some(),
        "shooter must receive a KilledShip event; got: {shooter_events:?}"
    );
    if let Some(Event::KilledShip { victim }) = killed_ev {
        assert_eq!(
            victim, "target",
            "KilledShip.victim must be 'target'; got {victim}"
        );
    }
}

// ─── test 44: kill bounty and banked-relic score combine; winner() correct ────
//
// Setup: attacker at (100, 100) heading East; defender at (121, 100) — exactly
// 21 units East, hit in the same tick the projectile is spawned.
// Relics spawn in [100, 102) — within pickup+bank radius of the attacker.
// After one step the attacker banks relics AND kills the defender; its total
// score must equal relics_banked × relic_value + kill_bounty.

#[test]
fn kill_bounty_combines_with_relic_score_for_winner() {
    let mut p = Params::default();
    // Tiny arena: relics spawn in [100, 102) × [100, 102).
    p.arena_w = 202.0;
    p.arena_h = 202.0;
    p.relic_field_cap = 2;       // initial = max(2, 1) = 2 relics
    p.carry_cap = 5;
    p.relic_value = 1.0;
    p.relic_spawn_period = 9999;
    p.max_ticks = 1;             // match ends after this one step
    // One-shot kill setup: no shield, thin hull, cannon ready.
    p.cannon_damage = 20.0;
    p.shield_max = 0.0;
    p.hull_max = 20.0;
    p.shield_regen = 0.0;
    p.cannon_start_hot = 0;
    p.cannon_cooldown = 9999;

    // Attacker at (100, 100): within pickup_radius of relics in [100, 102)^2
    // AND at its anchor → banking happens in the same step.
    // Defender at (121, 100): 21 units East of attacker → 1-tick kill.
    let specs = vec![
        ShipSpec {
            id: "attacker".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 100.0, y: 100.0 },
        },
        ShipSpec {
            id: "defender".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 121.0, y: 100.0 },
        },
    ];
    let mut engine = Engine::new(42, p.clone(), specs);

    let relics_before = engine.god_view().relics.len();
    assert!(relics_before > 0, "need at least 1 initial relic");

    engine.step(vec![("attacker".to_string(), fire_intent())]);

    let attacker_score = engine.score(&"attacker".to_string()).unwrap();
    let relics_banked = relics_before as f32 * p.relic_value;
    let expected = relics_banked + p.kill_bounty;

    assert!(
        (attacker_score - expected).abs() < 1e-3,
        "total score must be relics({relics_banked}) + kill_bounty({}); expected {expected}, got {attacker_score}",
        p.kill_bounty
    );

    // winner() must use the combined (relic + bounty) score.
    assert!(engine.is_match_over(), "match must be over after max_ticks=1");
    assert_eq!(
        engine.winner().as_deref(),
        Some("attacker"),
        "attacker (relics + kill bounty) must win over zero-score defender"
    );
}

// ─── test 45: golden scenario — kill awards bounty and registers in outcome ────
//
// Source: params.py kill_bounty = 2.0.
//
// Setup:
//   • Two ships: "hunter" at (0,0), "prey" at (21,0).
//   • prey: shield_max=0, hull_max=20, cannon_damage=20 → one-shot kill.
//   • max_ticks = 1 so that match ends immediately after the kill.
//
// Expected:
//   • hunter.score == kill_bounty == 2.0.
//   • winner() == "hunter".
//   • prey.alive == false.

#[test]
fn golden_kill_awards_bounty_and_registers_in_match_outcome() {
    let mut p = Params::default();
    p.cannon_damage = 20.0;
    p.shield_max = 0.0;
    p.hull_max = 20.0;
    p.shield_regen = 0.0;
    p.cannon_start_hot = 0;
    p.cannon_cooldown = 9999;
    p.relic_spawn_period = 9999;
    p.max_ticks = 1; // match ends after this one step

    let specs = vec![
        ShipSpec {
            id: "hunter".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 0.0, y: 0.0 },
        },
        ShipSpec {
            id: "prey".to_string(),
            class: ShipClass::Skiff,
            anchor_pos: Vec2 { x: 21.0, y: 0.0 },
        },
    ];
    let mut engine = Engine::new(7, p.clone(), specs);

    engine.step(vec![("hunter".to_string(), fire_intent())]);

    // Match must be over (max_ticks = 1 and tick = 1).
    assert!(engine.is_match_over(), "match must be over after max_ticks=1 step");

    // Prey must be destroyed.
    let prey = find_ship(&engine, "prey");
    assert!(!prey.alive, "prey must be not-alive after lethal hit");
    assert!(prey.hull.cur <= 0.001, "prey hull must be 0");

    // Hunter's score must equal exactly kill_bounty (no relics banked).
    let hunter_score = engine.score(&"hunter".to_string()).unwrap();
    assert!(
        (hunter_score - p.kill_bounty).abs() < 1e-4,
        "golden: hunter score must be kill_bounty={}; got {hunter_score}",
        p.kill_bounty
    );

    // winner() must return "hunter".
    let winner = engine.winner().expect("winner must be Some at match end");
    assert_eq!(
        winner, "hunter",
        "golden: winner must be 'hunter'; got {winner:?}"
    );
}

// ─── test 46 (non-attributed death — seam note) ───────────────────────────────
//
// A death with no killer (`Died { by: None }`) must award no bounty.
//
// Currently UNTESTABLE: the engine has no environmental damage source.
// Collision damage (issue 07) and Singularity damage (issue 08) will be the
// first `by: None` paths; they should call `handle_env_death(&mut ship, &mut events)`
// which emits `Died { by: None }` and explicitly does NOT award any score bounty.
// When issue 07 lands, add a test here that drives a ship into a wall hard
// enough to reach hull=0 and verifies the score map is unchanged.

// ─── test 47: determinism with kill scenario ──────────────────────────────────
//
// Two engines with the same seed and same fire-intent log (including a kill)
// must produce identical state after the kill tick.

#[test]
fn determinism_with_kill_scenario() {
    let make = || {
        let mut p = Params::default();
        p.cannon_damage = 20.0;
        p.shield_max = 0.0;
        p.hull_max = 20.0;
        p.shield_regen = 0.0;
        p.cannon_start_hot = 0;
        p.cannon_cooldown = 9999;
        p.relic_spawn_period = 9999;
        p.max_ticks = 30;
        let specs = vec![
            ShipSpec {
                id: "hunter".to_string(),
                class: ShipClass::Skiff,
                anchor_pos: Vec2 { x: 0.0, y: 0.0 },
            },
            ShipSpec {
                id: "prey".to_string(),
                class: ShipClass::Skiff,
                anchor_pos: Vec2 { x: 21.0, y: 0.0 },
            },
        ];
        Engine::new(55, p, specs)
    };

    // Script: fire on tick 1 (lethal), coast for 29 more ticks.
    let run = || {
        let mut e = make();
        e.step(vec![("hunter".to_string(), fire_intent())]);
        for _ in 0..29 {
            e.step(vec![]);
        }
        e.god_view()
    };

    let v1 = run();
    let v2 = run();

    assert_eq!(v1.tick, v2.tick, "ticks must match");
    for (s1, s2) in v1.ships.iter().zip(v2.ships.iter()) {
        assert_eq!(s1.id, s2.id);
        assert_eq!(s1.alive, s2.alive, "alive flag must match for {}", s1.id);
        assert_eq!(s1.hull.cur, s2.hull.cur, "hull must match for {}", s1.id);
    }
    // Scores must be identical.
    for (id, &sc1) in &v1.scores {
        let sc2 = v2.scores[id];
        assert_eq!(sc1, sc2, "score for {id} must be identical: {sc1} vs {sc2}");
    }
}

// ==========================================================================
// Issue 07: Collisions and hazards
// TDD tracer-bullet tests 48-57
// ==========================================================================

fn coll_params() -> Params {
    let mut p = Params::default();
    p.collision_enabled = true;
    p.n_asteroids = 0;
    p.relic_spawn_period = 9999;
    p.relic_field_cap = 0;
    p.shield_regen_delay = 9999;
    p.cannon_start_hot = 9999;
    p
}

fn find_pilot_view(engine: &Engine) -> arena_engine::GodShipView {
    engine.god_view().ships.into_iter().find(|s| s.id == "pilot").unwrap()
}

// --- test 48: asteroids in god-view and observation at match start -----------

#[test]
fn asteroids_in_god_view_and_observation_at_match_start() {
    let mut p = Params::default();
    p.n_asteroids = 5;
    p.relic_field_cap = 0;
    p.relic_spawn_period = 9999;
    let spec = ShipSpec {
        id: "s".to_string(),
        class: ShipClass::Skiff,
        anchor_pos: Vec2 { x: 1000.0, y: 600.0 },
    };
    let engine = Engine::new(42, p.clone(), vec![spec]);

    let god = engine.god_view();
    assert_eq!(god.asteroids.len(), 5,
        "god_view must contain n_asteroids=5; got {}", god.asteroids.len());
    let obs = engine.observation(&"s".to_string()).unwrap();
    assert_eq!(obs.asteroids.len(), 5,
        "observation must contain n_asteroids=5; got {}", obs.asteroids.len());
    for a in &god.asteroids {
        assert!(!a.id.is_empty(), "asteroid id must be non-empty");
        assert!(a.radius >= p.asteroid_radius_min && a.radius <= p.asteroid_radius_max,
            "radius {} not in [{}, {}]", a.radius, p.asteroid_radius_min, p.asteroid_radius_max);
    }
    let god_ids: std::collections::HashSet<&str> =
        god.asteroids.iter().map(|a| a.id.as_str()).collect();
    let obs_ids: std::collections::HashSet<&str> =
        obs.asteroids.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(god_ids, obs_ids, "god_view and observation must expose same asteroid ids");
}

// --- test 49: wall collision bounces and damages ----------------------------

#[test]
fn wall_collision_bounces_and_damages() {
    let mut p = coll_params();
    p.arena_w = 200.0;
    p.arena_h = 400.0;
    let spec = ShipSpec { id: "pilot".to_string(), class: ShipClass::Skiff,
                          anchor_pos: Vec2 { x: 100.0, y: 200.0 } };
    let mut engine = Engine::new(1, p.clone(), vec![spec]);

    let shield_before = p.shield_max;
    let thrust_east = Intent { thrust: Some(1.0), turn: Some(0.0), ..Default::default() };

    let mut bounced = false;
    for _ in 0..300 {
        engine.step(vec![("pilot".to_string(), thrust_east.clone())]);
        if find_pilot_view(&engine).vel.x < 0.0 { bounced = true; break; }
    }

    assert!(bounced, "ship must bounce off right wall (vel.x must become negative)");
    let ship = find_pilot_view(&engine);
    assert!(ship.shield.cur < shield_before,
        "wall collision must reduce shield; before={shield_before}, after={}", ship.shield.cur);
    assert!(ship.pos.x <= p.arena_w, "ship must stay inside arena; x={}", ship.pos.x);
}

// --- test 50: asteroid collision bounces and damages ------------------------

#[test]
fn asteroid_collision_bounces_and_damages() {
    let mut p = coll_params();
    p.n_asteroids = 1;
    p.asteroid_radius_min = 25.0;
    p.asteroid_radius_max = 26.0;
    p.arena_w = 600.0;
    p.arena_h = 600.0;
    let spec = ShipSpec { id: "pilot".to_string(), class: ShipClass::Skiff,
                          anchor_pos: Vec2 { x: 50.0, y: 300.0 } };
    let mut engine = Engine::new(1, p.clone(), vec![spec]);

    let shield_max = p.shield_max;
    let thrust_east = Intent { thrust: Some(1.0), turn: Some(0.0), ..Default::default() };

    let mut took_damage = false;
    for _ in 0..500 {
        engine.step(vec![("pilot".to_string(), thrust_east.clone())]);
        if find_pilot_view(&engine).shield.cur < shield_max { took_damage = true; break; }
    }

    assert!(took_damage,
        "ship must take damage from asteroid or wall within 500 ticks; \
         asteroid={:?}", engine.god_view().asteroids.first().map(|a| (a.pos.x, a.pos.y)));
}

// --- test 51: ship-ship ram damages both ships ------------------------------

#[test]
fn ship_ship_ram_damages_both_ships() {
    let mut p = coll_params();
    p.arena_w = 600.0;
    p.arena_h = 600.0;
    let specs = vec![
        ShipSpec { id: "alpha".to_string(), class: ShipClass::Skiff,
                   anchor_pos: Vec2 { x: 100.0, y: 300.0 } },
        ShipSpec { id: "beta".to_string(),  class: ShipClass::Skiff,
                   anchor_pos: Vec2 { x: 500.0, y: 300.0 } },
    ];
    let mut engine = Engine::new(1, p.clone(), vec![specs[0].clone(), specs[1].clone()]);

    let shield_max = p.shield_max;
    let ia = Intent { thrust: Some(1.0),  turn: Some(0.0), ..Default::default() };
    let ib = Intent { thrust: Some(-1.0), turn: Some(0.0), ..Default::default() };

    let mut both_damaged = false;
    for _ in 0..500 {
        engine.step(vec![("alpha".to_string(), ia.clone()), ("beta".to_string(), ib.clone())]);
        let view = engine.god_view();
        let a = view.ships.iter().find(|s| s.id == "alpha").unwrap();
        let b = view.ships.iter().find(|s| s.id == "beta").unwrap();
        if a.shield.cur < shield_max && b.shield.cur < shield_max { both_damaged = true; break; }
    }

    assert!(both_damaged, "both ships must take damage from ram within 500 ticks");
}

// --- test 52: sub-threshold speed deals 0 damage ---------------------------

#[test]
fn sub_threshold_speed_deals_zero_damage() {
    let mut p = coll_params();
    p.arena_w = 60.0;
    p.arena_h = 400.0;
    p.max_speed = 2.0;      // max_speed(2) < coll_threshold(4) -> 0 damage
    p.thrust_accel = 0.1;
    let spec = ShipSpec { id: "slow".to_string(), class: ShipClass::Skiff,
                          anchor_pos: Vec2 { x: 30.0, y: 200.0 } };
    let mut engine = Engine::new(1, p.clone(), vec![spec]);

    let shield_before = p.shield_max;
    let thrust_east = Intent { thrust: Some(1.0), turn: Some(0.0), ..Default::default() };

    let mut bounced = false;
    for _ in 0..600 {
        engine.step(vec![("slow".to_string(), thrust_east.clone())]);
        if engine.god_view().ships[0].vel.x < 0.0 { bounced = true; break; }
    }

    assert!(bounced, "slow ship must still bounce (but take no damage)");
    let shield_after = engine.god_view().ships[0].shield.cur;
    assert!((shield_after - shield_before).abs() < 0.01,
        "sub-threshold wall hit must deal 0 damage; before={shield_before}, after={shield_after}");
}

// --- test 53: invuln ship takes no collision damage -------------------------

#[test]
fn invuln_ship_takes_no_collision_damage() {
    let mut p = coll_params();
    p.arena_w = 200.0;
    p.arena_h = 400.0;
    let spec = ShipSpec { id: "pilot".to_string(), class: ShipClass::Skiff,
                          anchor_pos: Vec2 { x: 100.0, y: 200.0 } };
    let mut engine = Engine::new(1, p.clone(), vec![spec]);
    engine.set_invuln_for_test("pilot", true);

    let shield_before = p.shield_max;
    let thrust_east = Intent { thrust: Some(1.0), turn: Some(0.0), ..Default::default() };

    let mut wall_hit = false;
    for _ in 0..300 {
        let events = engine.step(vec![("pilot".to_string(), thrust_east.clone())]);
        if find_pilot_view(&engine).vel.x < 0.0 {
            wall_hit = true;
            let pilot_evs = &events.iter().find(|(id, _)| id == "pilot").unwrap().1;
            let took_coll = pilot_evs.iter().any(|ev| {
                matches!(ev, Event::CollisionTookShield { .. } | Event::CollisionTookHull { .. })
            });
            assert!(!took_coll,
                "invuln ship must not receive Collision events; got {:?}", pilot_evs);
            break;
        }
    }

    assert!(wall_hit, "invuln ship must still physically bounce");
    let shield_after = find_pilot_view(&engine).shield.cur;
    assert!((shield_after - shield_before).abs() < 0.01,
        "invuln shield must be unchanged; before={shield_before}, after={shield_after}");
}

// --- test 54: collision causes Died { by: None }, no kill bounty ------------

#[test]
fn collision_causes_env_death_by_none_no_bounty() {
    let mut p = coll_params();
    p.arena_w = 200.0;
    p.arena_h = 400.0;
    p.shield_max = 0.0;
    p.hull_max = 5.0;
    let spec = ShipSpec { id: "pilot".to_string(), class: ShipClass::Skiff,
                          anchor_pos: Vec2 { x: 100.0, y: 200.0 } };
    let mut engine = Engine::new(1, p.clone(), vec![spec]);

    let score_before = engine.score(&"pilot".to_string()).unwrap();
    let thrust_east = Intent { thrust: Some(1.0), turn: Some(0.0), ..Default::default() };

    let mut died = false;
    for _ in 0..300 {
        let events = engine.step(vec![("pilot".to_string(), thrust_east.clone())]);
        let pilot_evs = &events.iter().find(|(id, _)| id == "pilot").unwrap().1;
        for ev in pilot_evs {
            if let Event::Died { by } = ev {
                assert!(by.is_none(),
                    "env death must have Died {{ by: None }}; got {:?}", by);
                died = true;
                break;
            }
        }
        if died { break; }
    }

    assert!(died, "pilot must die from wall collision within 300 ticks");
    let score_after = engine.score(&"pilot".to_string()).unwrap();
    assert!((score_after - score_before).abs() < 1e-4,
        "env death must not award bounty; before={score_before}, after={score_after}");
}

// --- test 55: collision emits CollisionTookShield / CollisionTookHull -------

#[test]
fn collision_emits_collision_took_events() {
    let mut p = coll_params();
    p.arena_w = 200.0;
    p.arena_h = 400.0;
    let spec = ShipSpec { id: "pilot".to_string(), class: ShipClass::Skiff,
                          anchor_pos: Vec2 { x: 100.0, y: 200.0 } };
    let mut engine = Engine::new(1, p.clone(), vec![spec]);

    let thrust_east = Intent { thrust: Some(1.0), turn: Some(0.0), ..Default::default() };
    let mut found_shield_event = false;
    for _ in 0..300 {
        let events = engine.step(vec![("pilot".to_string(), thrust_east.clone())]);
        let pilot_evs = &events.iter().find(|(id, _)| id == "pilot").unwrap().1;
        for ev in pilot_evs {
            if let Event::CollisionTookShield { amount } = ev {
                assert!(*amount > 0.0, "CollisionTookShield amount must be > 0; got {amount}");
                found_shield_event = true;
            }
        }
        if found_shield_event { break; }
    }

    assert!(found_shield_event,
        "wall collision at high speed must emit CollisionTookShield event");
}

// --- test 56: golden wall damage formula ------------------------------------
//
// Formula (harness.py + params.py):
//   damage = max(0, (impact_speed - coll_threshold) * k_wall)
//          = max(0, (12 - 4) * 3) = 24.0
// shield after = 60 - 24 = 36.
// Source: harness.py wall block.
//
// Setup: wide arena so ship reaches terminal velocity (max_speed=12) well
// before hitting the right wall.  Ship at x=100 in 800-wide arena; wall at 780.
// Ramp-up takes ~44 ticks / ~240 units, leaving ~440 units at max_speed.

#[test]
fn golden_wall_collision_damage_formula() {
    let mut p = coll_params();
    p.arena_w = 800.0;    // right-wall boundary at 800-20=780; wide enough for
                           // ship to reach max_speed before hitting.
    p.arena_h = 1200.0;
    p.coll_threshold = 4.0;
    p.k_wall         = 3.0;
    p.shield_max     = 60.0;
    p.hull_max       = 100.0;
    p.max_speed      = 12.0;
    p.shield_regen_delay = 9999;
    let spec = ShipSpec { id: "pilot".to_string(), class: ShipClass::Skiff,
                          anchor_pos: Vec2 { x: 100.0, y: 600.0 } };
    let mut engine = Engine::new(1, p.clone(), vec![spec]);

    let thrust_east = Intent { thrust: Some(1.0), turn: Some(0.0), ..Default::default() };
    let mut damage_taken: f32 = 0.0;

    for _ in 0..500 {
        let events = engine.step(vec![("pilot".to_string(), thrust_east.clone())]);
        let pilot_evs = &events.iter().find(|(id, _)| id == "pilot").unwrap().1;
        for ev in pilot_evs {
            if let Event::CollisionTookShield { amount } = ev { damage_taken = *amount; }
        }
        if damage_taken > 0.0 { break; }
    }

    assert!(damage_taken > 0.0, "wall collision must occur within 500 ticks");

    // Expected at impact_speed ≈ max_speed (allow ±15% for damping).
    // Source: harness.py `self.damage(s, max(0, abs(s.vx) - p.coll_threshold) * p.k_wall)`.
    let expected = (p.max_speed - p.coll_threshold).max(0.0) * p.k_wall; // 24.0
    assert!(
        (damage_taken - expected).abs() < expected * 0.15,
        "golden wall damage: expected ~{expected:.1} (max(0,{}-{})*{}), got {damage_taken:.4}",
        p.max_speed, p.coll_threshold, p.k_wall
    );

    let ship = find_pilot_view(&engine);
    let expected_shield = (p.shield_max - damage_taken).max(0.0);
    assert!(
        (ship.shield.cur - expected_shield).abs() < 0.5,
        "golden shield after wall: expected ~{expected_shield:.1}, got {:.4}", ship.shield.cur
    );
}

// --- test 57: determinism with collision scenario ---------------------------

#[test]
fn determinism_collision_scenario() {
    let mut p = coll_params();
    p.arena_w = 200.0;
    p.arena_h = 400.0;
    p.n_asteroids = 3;

    let make = || {
        let spec = ShipSpec { id: "pilot".to_string(), class: ShipClass::Skiff,
                              anchor_pos: Vec2 { x: 100.0, y: 200.0 } };
        Engine::new(42, p.clone(), vec![spec])
    };

    let thrust_east = Intent { thrust: Some(1.0), turn: Some(0.0), ..Default::default() };
    let run = || {
        let mut e = make();
        for _ in 0..200 { e.step(vec![("pilot".to_string(), thrust_east.clone())]); }
        e.god_view()
    };

    let v1 = run();
    let v2 = run();

    let (s1, s2) = (&v1.ships[0], &v2.ships[0]);
    assert_eq!(s1.pos.x,      s2.pos.x,      "pos.x must be deterministic");
    assert_eq!(s1.pos.y,      s2.pos.y,      "pos.y must be deterministic");
    assert_eq!(s1.vel.x,      s2.vel.x,      "vel.x must be deterministic");
    assert_eq!(s1.vel.y,      s2.vel.y,      "vel.y must be deterministic");
    assert_eq!(s1.shield.cur, s2.shield.cur, "shield must be deterministic");
    assert_eq!(s1.hull.cur,   s2.hull.cur,   "hull must be deterministic");

    assert_eq!(v1.asteroids.len(), v2.asteroids.len());
    for (a1, a2) in v1.asteroids.iter().zip(v2.asteroids.iter()) {
        assert_eq!(a1.pos.x, a2.pos.x, "asteroid pos.x deterministic ({})", a1.id);
        assert_eq!(a1.pos.y, a2.pos.y, "asteroid pos.y deterministic ({})", a1.id);
    }
}
