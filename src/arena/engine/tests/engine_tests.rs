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
