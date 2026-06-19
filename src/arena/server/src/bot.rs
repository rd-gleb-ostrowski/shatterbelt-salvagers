//! Default Bot driver — fills any ship slot with no WS or WASM bot.
//!
//! Implements a simple Salvager heuristic: head toward the nearest Relic,
//! bank it at the Anchor when carrying enough, and fire opportunistically.
//! Good enough to ensure matches produce meaningful action while being
//! intentionally simple — real participant bots will be plugged in via the
//! `BotDriver` trait in issues 03 / 06.

use std::f32::consts::PI;

use arena_engine::{Intent, Observation, Params};

use crate::runner::BotDriver;

// ── DefaultBotDriver ─────────────────────────────────────────────────────────

/// The built-in Default Bot that fills unoccupied ship slots.
///
/// Uses a simple Salvager heuristic mirroring the engine's `harness::Policy::Salvager`:
/// - Navigate toward the nearest Relic, or bank at the Anchor when loaded.
/// - Fire the rune-cannon when roughly aimed at an enemy.
/// - Thrust at full when well-aimed; reduced thrust when turning hard.
///
/// This driver is stateless between ticks — all state lives in the engine's
/// `Observation`. A dead ship emits a no-op intent so the engine's respawn
/// counter can proceed undisturbed.
pub struct DefaultBotDriver {
    max_turn: f32,
    carry_cap: u32,
    proj_range: f32,
    shot_cost: f32,
}

impl DefaultBotDriver {
    /// Create a Default Bot calibrated to the match `Params`.
    pub fn new(params: &Params) -> Self {
        Self {
            max_turn: params.max_turn,
            carry_cap: params.carry_cap,
            proj_range: params.proj_range,
            shot_cost: params.shot_cost,
        }
    }
}

/// Normalise an angle to `[−π, π]`.
fn norm_angle(mut a: f32) -> f32 {
    while a > PI {
        a -= 2.0 * PI;
    }
    while a < -PI {
        a += 2.0 * PI;
    }
    a
}

fn dist(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

impl BotDriver for DefaultBotDriver {
    /// Compute a fresh intent from the current observation.
    ///
    /// Always returns `Some(intent)` — the Default Bot never misses a deadline.
    fn decide(&mut self, _tick: u32, obs: &Observation) -> Option<Intent> {
        let s = &obs.self_view;

        // Dead ship: no-op so the engine's respawn timer can proceed.
        if !s.alive {
            return Some(Intent::default());
        }

        // Navigation target:
        // 1. Return to Anchor to bank when loaded.
        // 2. Head toward nearest Relic.
        // 3. Chase nearest live enemy ship.
        // 4. Drift forward if nothing else.
        let target = if s.relics_carried >= self.carry_cap {
            obs.anchors
                .iter()
                .find(|a| a.ship_id == s.id)
                .map(|a| a.pos)
        } else if !obs.relics.is_empty() {
            obs.relics
                .iter()
                .min_by(|a, b| {
                    dist(a.pos.x, a.pos.y, s.pos.x, s.pos.y)
                        .partial_cmp(&dist(b.pos.x, b.pos.y, s.pos.x, s.pos.y))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|r| r.pos)
        } else {
            obs.ships
                .iter()
                .filter(|sh| sh.alive)
                .min_by(|a, b| {
                    dist(a.pos.x, a.pos.y, s.pos.x, s.pos.y)
                        .partial_cmp(&dist(b.pos.x, b.pos.y, s.pos.x, s.pos.y))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|sh| sh.pos)
        };

        let (turn, thrust) = if let Some(t) = target {
            let desired = (t.y - s.pos.y).atan2(t.x - s.pos.x);
            let err = norm_angle(desired - s.heading);
            let turn = (err / self.max_turn).clamp(-1.0, 1.0);
            let thrust = if err.abs() < 0.5 { 1.0_f32 } else { 0.3_f32 };
            (turn, thrust)
        } else {
            (0.1, 1.0)
        };

        // Fire when roughly aimed at the nearest live enemy and have aether.
        let fire = obs
            .ships
            .iter()
            .filter(|sh| sh.alive)
            .min_by(|a, b| {
                dist(a.pos.x, a.pos.y, s.pos.x, s.pos.y)
                    .partial_cmp(&dist(b.pos.x, b.pos.y, s.pos.x, s.pos.y))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .is_some_and(|e| {
                let d = dist(e.pos.x, e.pos.y, s.pos.x, s.pos.y);
                let ea = (e.pos.y - s.pos.y).atan2(e.pos.x - s.pos.x);
                let aim_err = norm_angle(ea - s.heading).abs();
                aim_err < 0.15 && d < self.proj_range && s.aether.cur >= self.shot_cost
            });

        Some(Intent {
            turn: Some(turn),
            thrust: Some(thrust),
            fire: Some(fire),
            sigil: None,
            sigil_target: None,
        })
    }
}
