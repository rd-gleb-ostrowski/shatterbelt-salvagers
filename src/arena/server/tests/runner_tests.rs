//! Integration tests for the match-runner loop (issue 01).
//!
//! Tests assert **observable behaviour** through the public API — no private
//! field inspection, no sleeping.  The stub transport (`ScriptedDriver`) and
//! `NoopPacer` let us drive ticks deterministically.

use arena_engine::{scale_drift, Intent, Params, ShipClass, ShipSpec, Vec2};
use arena_server::{
    bot::DefaultBotDriver,
    pacer::NoopPacer,
    runner::{BotDriver, MatchRunner},
};
use std::collections::HashMap;

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Build a `Params` with a shortened match and collisions disabled (faster tests).
fn test_params(max_ticks: u32) -> Params {
    Params {
        max_ticks,
        ..Params::default()
    }
}

/// Place N ships in a ring around the arena centre (mirrors harness.py placement).
fn make_specs(n: usize, params: &Params) -> Vec<ShipSpec> {
    use std::f32::consts::TAU;
    (0..n)
        .map(|i| {
            let ang = TAU * (i as f32) / (n as f32);
            let ax = (params.arena_w / 2.0 + ang.cos() * params.arena_w * 0.4)
                .clamp(50.0, params.arena_w - 50.0);
            let ay = (params.arena_h / 2.0 + ang.sin() * params.arena_h * 0.4)
                .clamp(50.0, params.arena_h - 50.0);
            ShipSpec {
                id: format!("ship-{i}"),
                class: ShipClass::Skiff,
                anchor_pos: Vec2::new(ax, ay),
            }
        })
        .collect()
}

/// A scripted driver for testing.
///
/// On each tick returns the pre-configured intent (or `None` to model a late /
/// missing intent).  Intents are keyed by the tick on which they are available —
/// a tick with no entry (or explicit `None`) models a deadline miss; the engine
/// carries over the ship's previous intent.
pub struct ScriptedDriver {
    /// `tick → Some(intent)` means return that intent on that tick.
    /// `tick → None` means return `None` (deadline miss, previous persists).
    /// Missing ticks fall through to `default_intent`.
    script: HashMap<u32, Option<Intent>>,
    /// Returned for ticks not in the script.
    default_intent: Option<Intent>,
}

impl ScriptedDriver {
    /// Always return `None` (no intent, ever — previous always persists).
    pub fn silent() -> Self {
        Self {
            script: HashMap::new(),
            default_intent: None,
        }
    }

    /// Return the given intent every tick (always fresh).
    pub fn always(intent: Intent) -> Self {
        Self {
            script: HashMap::new(),
            default_intent: Some(intent),
        }
    }

    /// Configure one entry: return `intent` (or `None` for a deadline miss)
    /// on exactly tick `t`.
    pub fn on_tick(mut self, t: u32, intent: Option<Intent>) -> Self {
        self.script.insert(t, intent);
        self
    }
}

impl BotDriver for ScriptedDriver {
    fn decide(&mut self, tick: u32, _obs: &arena_engine::Observation) -> Option<Intent> {
        if let Some(entry) = self.script.get(&tick) {
            entry.clone()
        } else {
            self.default_intent.clone()
        }
    }
}

// ── Test 1: all-Default-Bot match runs to completion ─────────────────────────
//
// RED → GREEN: construct a runner with 2 Default Bot drivers and a NoopPacer;
// assert run_to_completion returns when the engine is done (ticks == max_ticks).

#[test]
fn all_default_bots_match_runs_to_completion() {
    let params = scale_drift(&test_params(400), 2);
    let specs = make_specs(2, &params);
    let drivers: Vec<Box<dyn BotDriver>> = vec![
        Box::new(DefaultBotDriver::new(&params)),
        Box::new(DefaultBotDriver::new(&params)),
    ];
    let mut runner = MatchRunner::new(42, params.clone(), specs, drivers, Box::new(NoopPacer));

    let outcome = runner.run_to_completion();

    assert_eq!(
        outcome.ticks, params.max_ticks,
        "match must run for exactly max_ticks ticks"
    );
    assert_eq!(
        outcome.scores.len(),
        2,
        "one score entry per ship"
    );
}

// ── Test 2: runner advances tick exactly once per step_once call ─────────────

#[test]
fn tick_advances_one_per_step() {
    let params = scale_drift(&test_params(100), 2);
    let specs = make_specs(2, &params);
    let drivers: Vec<Box<dyn BotDriver>> = vec![
        Box::new(DefaultBotDriver::new(&params)),
        Box::new(DefaultBotDriver::new(&params)),
    ];
    let mut runner = MatchRunner::new(1, params, specs, drivers, Box::new(NoopPacer));

    assert_eq!(runner.engine().tick(), 0, "starts at tick 0");
    runner.step_once();
    assert_eq!(runner.engine().tick(), 1, "tick 1 after first step");
    runner.step_once();
    assert_eq!(runner.engine().tick(), 2, "tick 2 after second step");
}

// ── Test 3: no intent this tick → previous intent persists ───────────────────
//
// Ship-0 sends turn=1.0 on tick 0, then goes silent.
// Ship-1 always sends turn=0.0.
// After tick 1 (where ship-0 sent nothing), ship-0 should still be rotating
// (heading changed again), because the engine carries over turn=1.0.

#[test]
fn no_intent_persists_previous_turn() {
    let params = scale_drift(&test_params(50), 2);
    let specs = make_specs(2, &params);

    // Ship-0: sends turn=1.0 exactly on tick 0, then nothing.
    let driver0 = ScriptedDriver::silent().on_tick(
        0,
        Some(Intent {
            turn: Some(1.0),
            thrust: Some(0.0),
            fire: Some(false),
            sigil: None,
            sigil_target: None,
        }),
    );
    // Ship-1: always sends turn=0.0 thrust=0.
    let driver1 = ScriptedDriver::always(Intent {
        turn: Some(0.0),
        thrust: Some(0.0),
        fire: Some(false),
        sigil: None,
        sigil_target: None,
    });
    let drivers: Vec<Box<dyn BotDriver>> =
        vec![Box::new(driver0), Box::new(driver1)];
    let mut runner = MatchRunner::new(10, params, specs, drivers, Box::new(NoopPacer));

    // Tick 0: ship-0 supplies turn=1.0.
    runner.step_once();
    let heading_after_tick0 = runner
        .engine()
        .god_view()
        .ships
        .iter()
        .find(|s| s.id == "ship-0")
        .map(|s| s.heading)
        .expect("ship-0 exists");

    // Tick 1: ship-0 supplies nothing — previous turn=1.0 should persist.
    runner.step_once();
    let heading_after_tick1 = runner
        .engine()
        .god_view()
        .ships
        .iter()
        .find(|s| s.id == "ship-0")
        .map(|s| s.heading)
        .expect("ship-0 exists");

    assert_ne!(
        heading_after_tick0, heading_after_tick1,
        "ship-0 must keep turning after a tick with no supplied intent"
    );
}

// ── Test 4: deadline miss → previous persists; fresh intent applied next tick ──
//
// Scenario:
//   tick 0: ship-0 sends turn=1.0  → heading starts changing
//   tick 1: ship-0 misses deadline (None) → engine carries over turn=1.0;
//           heading continues to change (previous intent still active)
//   tick 2: ship-0 sends turn=0.0 (the "queued late intent from tick-1") → applied;
//           heading stops changing from this point
//   tick 3: no intent → turn=0.0 persists; heading stays constant
//
// This exercises PROTOCOL §2 / ADR-0003: missed deadline ⇒ previous intent persists.

#[test]
fn late_intent_applied_next_tick() {
    let params = scale_drift(&test_params(50), 2);
    let specs = make_specs(2, &params);

    let driver0 = ScriptedDriver::silent()
        // tick 0: establish turn=1.0
        .on_tick(
            0,
            Some(Intent {
                turn: Some(1.0),
                thrust: Some(0.0),
                fire: Some(false),
                sigil: None,
                sigil_target: None,
            }),
        )
        // tick 1: deadline miss (None) → engine carries over turn=1.0
        .on_tick(1, None)
        // tick 2: late intent now available; turn=0.0 applied, heading stops
        .on_tick(
            2,
            Some(Intent {
                turn: Some(0.0),
                thrust: Some(0.0),
                fire: Some(false),
                sigil: None,
                sigil_target: None,
            }),
        );
    let driver1 = ScriptedDriver::always(Intent {
        turn: Some(0.0),
        thrust: Some(0.0),
        fire: Some(false),
        sigil: None,
        sigil_target: None,
    });
    let drivers: Vec<Box<dyn BotDriver>> =
        vec![Box::new(driver0), Box::new(driver1)];
    let mut runner = MatchRunner::new(20, params, specs, drivers, Box::new(NoopPacer));

    let ship0_heading = |runner: &MatchRunner| {
        runner
            .engine()
            .god_view()
            .ships
            .iter()
            .find(|s| s.id == "ship-0")
            .map(|s| s.heading)
            .expect("ship-0 exists")
    };

    runner.step_once(); // tick 0: apply turn=1.0
    let h0 = ship0_heading(&runner);

    runner.step_once(); // tick 1: deadline miss → engine carries over turn=1.0
    let h1 = ship0_heading(&runner);

    runner.step_once(); // tick 2: fresh turn=0.0 applied → heading stops
    let h2 = ship0_heading(&runner);

    runner.step_once(); // tick 3: turn=0.0 persists → heading still constant
    let h3 = ship0_heading(&runner);

    // Heading changes on tick 1 because previous turn=1.0 is carried over.
    assert_ne!(h0, h1, "heading must change on tick 1 (persisted turn=1.0 carried over)");
    // Heading STOPS changing on tick 2 because fresh turn=0.0 is applied.
    assert_eq!(h1, h2, "heading must stop changing on tick 2 (late intent turn=0.0 applied)");
    // Heading stays constant on tick 3 (turn=0.0 persists).
    assert_eq!(h2, h3, "heading stays constant on tick 3 (turn=0.0 persists)");
}

// ── Test 5: fresh intent before deadline IS applied this tick ─────────────────
//
// Scenario:
//   tick 0: ship-0 sends nothing → default turn=0, heading unchanged
//   tick 1: ship-0 sends fresh turn=1.0 → applied this tick; heading changes
//   tick 2: ship-0 sends nothing → turn=1.0 persists; heading continues changing
//
// This confirms the positive case: a fresh intent IS applied in the tick it arrives.

#[test]
fn fresh_intent_applied_this_tick() {
    let params = scale_drift(&test_params(50), 2);
    let specs = make_specs(2, &params);

    let driver0 = ScriptedDriver::silent().on_tick(
        1,
        Some(Intent {
            turn: Some(1.0),
            thrust: Some(0.0),
            fire: Some(false),
            sigil: None,
            sigil_target: None,
        }),
    );
    let driver1 = ScriptedDriver::always(Intent {
        turn: Some(0.0),
        thrust: Some(0.0),
        fire: Some(false),
        sigil: None,
        sigil_target: None,
    });
    let drivers: Vec<Box<dyn BotDriver>> =
        vec![Box::new(driver0), Box::new(driver1)];
    let mut runner = MatchRunner::new(30, params, specs, drivers, Box::new(NoopPacer));

    let ship0_heading = |runner: &MatchRunner| {
        runner
            .engine()
            .god_view()
            .ships
            .iter()
            .find(|s| s.id == "ship-0")
            .map(|s| s.heading)
            .expect("ship-0 exists")
    };

    let initial = ship0_heading(&runner);

    runner.step_once(); // tick 0: no intent → turn=0 → heading unchanged
    let h0 = ship0_heading(&runner);

    runner.step_once(); // tick 1: fresh turn=1.0 applied → heading changes
    let h1 = ship0_heading(&runner);

    runner.step_once(); // tick 2: turn=1.0 persists → heading continues changing
    let h2 = ship0_heading(&runner);

    // Tick 0 applied default turn=0 → heading stays at initial value.
    assert_eq!(initial, h0, "heading unchanged on tick 0 (no fresh intent, turn=0 default)");
    // Tick 1 applied fresh turn=1.0 → heading changed.
    assert_ne!(h0, h1, "heading changes on tick 1 (fresh turn=1.0 applied)");
    // Tick 2 persists turn=1.0 → heading continues changing.
    assert_ne!(h1, h2, "heading continues changing on tick 2 (persisted turn=1.0)");
}

// ── Test 6: determinism — same seed + same script → identical result ──────────

#[test]
fn determinism_same_seed_same_result() {
    let params = scale_drift(&test_params(200), 2);
    let specs = make_specs(2, &params);

    let make_drivers = |params: &Params| -> Vec<Box<dyn BotDriver>> {
        vec![
            Box::new(DefaultBotDriver::new(params)),
            Box::new(DefaultBotDriver::new(params)),
        ]
    };

    let mut runner_a =
        MatchRunner::new(99, params.clone(), specs.clone(), make_drivers(&params), Box::new(NoopPacer));
    let outcome_a = runner_a.run_to_completion();
    let view_a = runner_a.engine().god_view();

    let mut runner_b =
        MatchRunner::new(99, params.clone(), specs.clone(), make_drivers(&params), Box::new(NoopPacer));
    let outcome_b = runner_b.run_to_completion();
    let view_b = runner_b.engine().god_view();

    assert_eq!(outcome_a.ticks, outcome_b.ticks, "same ticks");
    assert_eq!(outcome_a.winner, outcome_b.winner, "same winner");
    assert_eq!(view_a, view_b, "identical final god_view");
}
