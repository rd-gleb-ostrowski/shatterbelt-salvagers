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
