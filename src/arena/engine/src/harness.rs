//! Headless balance harness for Shatterbelt Salvagers.
//!
//! Mirrors `src/arena/balance/harness.py`:
//! - Scripted stub-bot policies (Salvager, Aggressor) that produce deterministic
//!   `Intent` sequences from per-tick `Observation`s.
//! - `run_match`: run one full match with given policies and return aggregate stats.
//! - `run_batch`: run N seeded matches and compute aggregate stats over the batch.
//! - `replay_match`: given a seed + applied-intent log, reproduce a match exactly.
//!
//! The replay guarantee is the headline deliverable of issue 11: a recorded
//! `(seed, scaled_params, specs, intent_log)` deterministically reproduces an
//! identical final `GodView`, score, and winner.

use std::f32::consts::{PI, TAU};

use crate::engine::{scale_drift, Engine, IntentFrame};
use crate::intent::Intent;
use crate::observation::{GodView, Observation};
use crate::params::Params;
use crate::types::{Event, ShipClass, ShipId, ShipSpec, Sigil, Vec2};

// ─── Public API types ─────────────────────────────────────────────────────────

/// Scripted stub-bot policy — a deterministic heuristic that mirrors harness.py.
///
/// A `Policy` maps each tick's `Observation` to an `Intent` with no mutable state
/// between ticks; the engine state (position, aether, …) is the only memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Policy {
    /// Collects Relics and banks them at the Anchor; fires the rune-cannon
    /// opportunistically when an enemy is in the sights.
    Salvager,
    /// Hunts the nearest enemy and fires aggressively; collects Relics only
    /// when no enemy is reachable.
    Aggressor,
}

/// Result of one completed match, plus everything needed to replay it exactly.
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// Final banked score per ship, in ship-construction order.
    pub scores: Vec<(ShipId, f32)>,
    /// The ship with the highest score, or `None` on a tie.
    pub winner: Option<ShipId>,
    /// Total kill count across all ships (sum of `KilledShip` events).
    pub total_kills: u32,
    /// Score of the leading ship.
    pub leader_score: f32,
    /// `leader_score − second_place_score`.
    pub margin: f32,
    /// `true` when `leader_score > 0` AND `margin ≥ 0.20 × leader_score`.
    pub decisive: bool,
    /// `true` when every ship ended with score 0.
    pub shutout: bool,
    /// Final god-mode snapshot (tick == max_ticks).
    pub final_god_view: GodView,
    /// Seed used to construct the engine.
    pub seed: u64,
    /// The *scaled* `Params` used in this match (after `scale_drift`).
    /// Required to replay the match identically.
    pub params: Params,
    /// The exact `ShipSpec` list used to construct the engine.
    /// Required to replay the match identically.
    pub specs: Vec<ShipSpec>,
    /// Full applied-intent log — one `IntentFrame` per completed tick.
    /// Combined with `seed` and `params` this is sufficient for exact replay.
    pub intent_log: Vec<IntentFrame>,
}

/// Aggregate statistics over a batch of matches.
#[derive(Debug, Clone)]
pub struct BatchStats {
    /// Number of matches in the batch.
    pub n: usize,
    /// Mean leader score across all matches.
    pub leader_mean: f32,
    /// Maximum leader score seen in any match.
    pub leader_max: f32,
    /// Mean lead margin (leader − second).
    pub margin_mean: f32,
    /// Mean total kills per match.
    pub kills_mean: f32,
    /// Percentage of matches that were "decisive" (margin ≥ 20% of leader).
    pub decisive_pct: f32,
    /// Percentage of matches where every ship scored 0 (no relic banked, no kills).
    pub shutout_pct: f32,
}

// ─── Public functions ─────────────────────────────────────────────────────────

/// Run one full match with scripted policies, returning aggregate stats and
/// everything needed to replay it.
///
/// `params` are scaled via `scale_drift` for the number of ships before the
/// engine is constructed, mirroring `harness.py::run_match → scale_for`.
/// Ships are placed in a circle around the arena centre at 0.4 × arena-radius,
/// identical to `harness.py::World.__init__`.
pub fn run_match(params: Params, policies: &[Policy], seed: u64) -> MatchResult {
    let n = policies.len();
    let scaled = scale_drift(&params, n);
    let specs = make_specs(policies, &scaled);
    run_match_internal(scaled, specs, policies, seed)
}

/// Run `n_matches` seeded matches (seeds `seed0 .. seed0 + n_matches`) and
/// compute aggregate stats over the batch.
///
/// Mirrors `harness.py::run_batch`.
pub fn run_batch(
    params: &Params,
    policies: &[Policy],
    n_matches: usize,
    seed0: u64,
) -> BatchStats {
    let mut leaders = Vec::with_capacity(n_matches);
    let mut margins = Vec::with_capacity(n_matches);
    let mut kills = Vec::with_capacity(n_matches);
    let mut decisive = 0usize;
    let mut shutout = 0usize;

    for i in 0..n_matches {
        let r = run_match(params.clone(), policies, seed0 + i as u64);
        leaders.push(r.leader_score);
        margins.push(r.margin);
        kills.push(r.total_kills);
        if r.decisive {
            decisive += 1;
        }
        if r.shutout {
            shutout += 1;
        }
    }

    let n_f = n_matches as f32;
    BatchStats {
        n: n_matches,
        leader_mean: leaders.iter().sum::<f32>() / n_f,
        leader_max: leaders.iter().cloned().fold(0.0_f32, f32::max),
        margin_mean: margins.iter().sum::<f32>() / n_f,
        kills_mean: kills.iter().sum::<u32>() as f32 / n_f,
        decisive_pct: 100.0 * decisive as f32 / n_f,
        shutout_pct: 100.0 * shutout as f32 / n_f,
    }
}

/// Replay a recorded match exactly.
///
/// Given the exact `(scaled_params, specs, seed, intent_log)` from a prior
/// `MatchResult`, reconstruct the engine and replay every tick from the
/// intent log.  The returned `MatchResult` will have an identical `final_god_view`,
/// `scores`, and `winner` to the original run — proving replay parity.
///
/// `params` **must** be the already-scaled params stored in `MatchResult::params`
/// (not the base params).  Similarly, `specs` must be `MatchResult::specs`.
pub fn replay_match(
    params: Params,
    specs: Vec<ShipSpec>,
    seed: u64,
    intent_log: &[IntentFrame],
) -> MatchResult {
    let ship_ids: Vec<ShipId> = specs.iter().map(|s| s.id.clone()).collect();
    let mut engine = Engine::new(seed, params.clone(), specs.clone());
    let mut total_kills: u32 = 0;

    for frame in intent_log {
        let events_per_ship = engine.step(frame.clone());
        for (_id, evs) in &events_per_ship {
            for ev in evs {
                if matches!(ev, Event::KilledShip { .. }) {
                    total_kills += 1;
                }
            }
        }
    }

    build_result(engine, ship_ids, total_kills, seed, params, specs)
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Core match runner: given already-scaled params, specs, and policy assignments.
fn run_match_internal(
    scaled: Params,
    specs: Vec<ShipSpec>,
    policies: &[Policy],
    seed: u64,
) -> MatchResult {
    let ship_ids: Vec<ShipId> = specs.iter().map(|s| s.id.clone()).collect();
    let mut engine = Engine::new(seed, scaled.clone(), specs.clone());
    let mut total_kills: u32 = 0;

    while !engine.is_match_over() {
        // Each ship's policy decides from its own Observation this tick.
        let intents: Vec<(ShipId, Intent)> = ship_ids
            .iter()
            .enumerate()
            .map(|(idx, id)| {
                let obs = engine
                    .observation(id)
                    .expect("observation for valid ship id");
                let intent = decide(policies[idx], &obs, &scaled);
                (id.clone(), intent)
            })
            .collect();

        let events_per_ship = engine.step(intents);
        for (_id, evs) in &events_per_ship {
            for ev in evs {
                if matches!(ev, Event::KilledShip { .. }) {
                    total_kills += 1;
                }
            }
        }
    }

    build_result(engine, ship_ids, total_kills, seed, scaled, specs)
}

/// Build a `MatchResult` from a completed engine.
fn build_result(
    engine: Engine,
    ship_ids: Vec<ShipId>,
    total_kills: u32,
    seed: u64,
    params: Params,
    specs: Vec<ShipSpec>,
) -> MatchResult {
    let final_god_view = engine.god_view();
    let winner = engine.winner();
    let intent_log = engine.intent_log().to_vec();

    let scores: Vec<(ShipId, f32)> = ship_ids
        .iter()
        .map(|id| (id.clone(), engine.score(id).unwrap_or(0.0)))
        .collect();

    let leader_score = scores
        .iter()
        .map(|(_, s)| *s)
        .fold(f32::NEG_INFINITY, f32::max)
        .max(0.0);

    let mut sorted: Vec<f32> = scores.iter().map(|(_, s)| *s).collect();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let second = sorted.get(1).copied().unwrap_or(0.0);
    let margin = leader_score - second;
    let decisive = leader_score > 0.0 && margin >= 0.2 * leader_score;
    let shutout = leader_score <= 0.0;

    MatchResult {
        scores,
        winner,
        total_kills,
        leader_score,
        margin,
        decisive,
        shutout,
        final_god_view,
        seed,
        params,
        specs,
        intent_log,
    }
}

/// Create `ShipSpec` list placing ships in a circle around the arena centre.
///
/// Mirrors `harness.py::World.__init__` anchor placement:
/// ```text
/// ang = TAU * i / n
/// ax  = arena_w/2 + cos(ang) * arena_w * 0.4   (clamped to [50, arena_w-50])
/// ay  = arena_h/2 + sin(ang) * arena_h * 0.4   (clamped to [50, arena_h-50])
/// ```
fn make_specs(policies: &[Policy], params: &Params) -> Vec<ShipSpec> {
    let n = policies.len();
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

/// Normalise an angle to `[−π, π]`.
fn norm_angle(mut a: f32) -> f32 {
    while a > PI {
        a -= TAU;
    }
    while a < -PI {
        a += TAU;
    }
    a
}

/// Euclidean distance squared between two positions.
fn dist_sq(a: Vec2, b: Vec2) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

// ─── Bot decision logic ───────────────────────────────────────────────────────

/// Map one tick's `Observation` to an `Intent` for the given `Policy`.
///
/// Mirrors `World.decide` + `World.decide_sigil` in harness.py exactly:
/// - Navigation: proportional turn controller (err / max_turn, clamped ±1).
/// - Thrust:  1.0 when heading error < 0.5 rad, 0.3 otherwise.
/// - Fire:    when aimed at nearest enemy within proj_range and enough aether.
/// - Sigil:   discharged using the harness.py heuristic triggers.
fn decide(policy: Policy, obs: &Observation, params: &Params) -> Intent {
    let s = &obs.self_view;

    // Dead ships send no-op intents; the engine ignores them.
    if !s.alive {
        return Intent::default();
    }

    // Nearest alive enemy (from visible other ships).
    let nearest_enemy = obs
        .ships
        .iter()
        .filter(|o| o.alive)
        .min_by(|a, b| {
            dist_sq(a.pos, s.pos)
                .partial_cmp(&dist_sq(b.pos, s.pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    // Own anchor position.
    let anchor = obs
        .anchors
        .iter()
        .find(|a| a.ship_id == s.id)
        .map(|a| a.pos)
        .unwrap_or(s.pos);

    // Navigation target — mirrors harness.py `decide()`.
    let (tx, ty) = match policy {
        Policy::Aggressor => {
            if let Some(e) = nearest_enemy {
                (e.pos.x, e.pos.y)
            } else {
                (anchor.x, anchor.y)
            }
        }
        Policy::Salvager => {
            if s.relics_carried >= params.carry_cap
                || (s.relics_carried > 0 && obs.relics.is_empty())
            {
                // Return to anchor to bank.
                (anchor.x, anchor.y)
            } else if !obs.relics.is_empty() {
                // Head to nearest relic.
                let r = obs
                    .relics
                    .iter()
                    .min_by(|a, b| {
                        dist_sq(a.pos, s.pos)
                            .partial_cmp(&dist_sq(b.pos, s.pos))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap();
                (r.pos.x, r.pos.y)
            } else if let Some(e) = nearest_enemy {
                (e.pos.x, e.pos.y)
            } else {
                (anchor.x, anchor.y)
            }
        }
    };

    // Proportional turn controller (mirrors harness.py).
    let desired = (ty - s.pos.y).atan2(tx - s.pos.x);
    let err = norm_angle(desired - s.heading);
    let turn = (err / params.max_turn).clamp(-1.0, 1.0);
    let thrust = if err.abs() < 0.5 { 1.0_f32 } else { 0.3_f32 };

    // Fire when aimed at the nearest enemy within range.
    let fire = nearest_enemy.map_or(false, |e| {
        let ea = (e.pos.y - s.pos.y).atan2(e.pos.x - s.pos.x);
        let d = dist_sq(e.pos, s.pos).sqrt();
        let aim_err = norm_angle(ea - s.heading).abs();
        aim_err < 0.12 && d < params.proj_range && s.aether.cur >= params.shot_cost
    });

    // Sigil decision.
    let (use_sigil, sigil_target) = decide_sigil(policy, obs, nearest_enemy, params);

    Intent {
        turn: Some(turn),
        thrust: Some(thrust),
        fire: Some(fire),
        sigil: if use_sigil { Some(true) } else { None },
        sigil_target,
    }
}

/// Decide whether to discharge the held Sigil and where to aim it.
///
/// Mirrors `World.decide_sigil` in harness.py — greedy triggers based on local
/// conditions (distance to nearest enemy, relic proximity, etc.).
fn decide_sigil(
    policy: Policy,
    obs: &Observation,
    nearest_enemy: Option<&crate::observation::OtherShipView>,
    params: &Params,
) -> (bool, Option<Vec2>) {
    let s = &obs.self_view;
    let sigil = match &s.sigil {
        Some(sg) => sg,
        None => return (false, None),
    };

    let d = nearest_enemy
        .map(|e| dist_sq(e.pos, s.pos).sqrt())
        .unwrap_or(f32::MAX);

    match sigil {
        Sigil::Afterburner => {
            let use_it = (s.relics_carried > 0 && d < 300.0)
                || (policy == Policy::Aggressor && d > 500.0);
            (use_it, None)
        }
        Sigil::Bulwark => {
            let use_it = s.shield.cur <= 0.0 && d < 450.0;
            (use_it, None)
        }
        Sigil::ArcLance => {
            if let Some(e) = nearest_enemy {
                if d < params.proj_range {
                    let ea = (e.pos.y - s.pos.y).atan2(e.pos.x - s.pos.x);
                    let err = norm_angle(ea - s.heading).abs();
                    if err < 0.2 {
                        return (true, Some(e.pos));
                    }
                }
            }
            (false, None)
        }
        Sigil::AetherMine => {
            let use_it = s.relics_carried > 0 && d < 200.0;
            (use_it, None)
        }
        Sigil::Singularity => {
            // If 3+ relics are close, pull them toward their centroid.
            let nearby: Vec<Vec2> = obs
                .relics
                .iter()
                .filter(|r| dist_sq(r.pos, s.pos) < 250.0 * 250.0)
                .map(|r| r.pos)
                .collect();
            if nearby.len() >= 3 {
                let cx = nearby.iter().map(|r| r.x).sum::<f32>() / nearby.len() as f32;
                let cy = nearby.iter().map(|r| r.y).sum::<f32>() / nearby.len() as f32;
                return (true, Some(Vec2::new(cx, cy)));
            }
            // Otherwise pull toward nearest enemy.
            if let Some(e) = nearest_enemy {
                if d < 250.0 {
                    return (true, Some(e.pos));
                }
            }
            (false, None)
        }
    }
}
