use std::collections::HashMap;

use rand::SeedableRng;
use rand_pcg::Pcg64;

use crate::intent::Intent;
use crate::observation::{GodShipView, GodView, Observation, OtherShipView, SelfView};
use crate::params::Params;
use crate::types::*;

// ─── Persisted intent state ───────────────────────────────────────────────────

/// The concrete values that persist between ticks when a bot omits fields.
/// Sigil is one-shot and therefore not persisted here.
#[derive(Debug, Clone, Default)]
struct PersistedIntent {
    turn: f32,
    thrust: f32,
    fire: bool,
}

// ─── Internal ship state ──────────────────────────────────────────────────────

struct ShipState {
    id: ShipId,
    class: ShipClass,
    alive: bool,
    invuln: bool,
    pos: Vec2,
    vel: Vec2,
    heading: f32,
    ang_vel: f32,
    hull: Resource,
    shield: Resource,
    aether: Resource,
    sigil: Option<Sigil>,
    cannon_cooldown: u32,
    relics_carried: u32,
    anchor_pos: Vec2,
    persisted: PersistedIntent,
}

impl ShipState {
    /// Merge an incoming `Intent` into this ship's persisted intent state.
    fn merge_intent(&mut self, intent: &Intent) {
        if let Some(v) = intent.turn {
            self.persisted.turn = v;
        }
        if let Some(v) = intent.thrust {
            self.persisted.thrust = v;
        }
        if let Some(v) = intent.fire {
            self.persisted.fire = v;
        }
    }

    /// Snapshot the applied (persisted) state as a fully-specified `Intent`
    /// for the intent log.  All Options are `Some` so replayers know exactly
    /// what was applied each tick without re-running the merge logic.
    fn applied_intent(&self) -> Intent {
        Intent {
            turn: Some(self.persisted.turn),
            thrust: Some(self.persisted.thrust),
            fire: Some(self.persisted.fire),
            sigil: None,       // one-shot; not persisted
            sigil_target: None,
        }
    }

    fn to_god_view(&self) -> GodShipView {
        GodShipView {
            id: self.id.clone(),
            class: self.class.clone(),
            alive: self.alive,
            invuln: self.invuln,
            pos: self.pos,
            vel: self.vel,
            heading: self.heading,
            ang_vel: self.ang_vel,
            hull: self.hull,
            shield: self.shield,
            aether: self.aether,
            sigil: self.sigil.clone(),
            cannon_cooldown: self.cannon_cooldown,
            relics_carried: self.relics_carried,
        }
    }

    fn to_self_view(&self) -> SelfView {
        SelfView {
            id: self.id.clone(),
            class: self.class.clone(),
            alive: self.alive,
            invuln: self.invuln,
            pos: self.pos,
            vel: self.vel,
            heading: self.heading,
            ang_vel: self.ang_vel,
            hull: self.hull,
            shield: self.shield,
            aether: self.aether,
            sigil: self.sigil.clone(),
            cannon_cooldown: self.cannon_cooldown,
            relics_carried: self.relics_carried,
        }
    }

    fn to_other_view(&self) -> OtherShipView {
        OtherShipView {
            id: self.id.clone(),
            class: self.class.clone(),
            alive: self.alive,
            invuln: self.invuln,
            pos: self.pos,
            vel: self.vel,
            heading: self.heading,
            hull: self.hull,
            shield: self.shield,
            relics_carried: self.relics_carried,
            // aether and sigil are intentionally absent
        }
    }
}

// ─── Intent log ──────────────────────────────────────────────────────────────

/// One tick's worth of applied intents, one entry per ship.
/// All intent fields are `Some` — this records what was *actually applied*,
/// not just what the bot sent.  Use this for exact match replay.
pub type IntentFrame = Vec<(ShipId, Intent)>;

// ─── Engine ──────────────────────────────────────────────────────────────────

/// The deterministic, headless heart of a Shatterbelt Salvagers Match.
///
/// Constructed from `(seed, params, ships)`, advanced one tick at a time via
/// `step(intents) -> events`, and queried via `observation` / `god_view`.
///
/// Zero dependencies on networking, WASM, HTTP, ladder, or auth.
pub struct Engine {
    seed: u64,
    params: Params,
    tick: u32,
    ships: Vec<ShipState>,
    scores: HashMap<ShipId, f32>,
    intent_log: Vec<IntentFrame>,
    /// Seeded RNG — used for spawning/Sigil assignment in later issues.
    rng: Pcg64,
}

impl Engine {
    /// Construct a new engine in its initial state.
    ///
    /// Each ship starts at its `anchor_pos` with zero velocity, full Hull /
    /// Shield / Aether, no Sigil, and the cannon on its start-hot cooldown.
    pub fn new(seed: u64, params: Params, specs: Vec<ShipSpec>) -> Self {
        let rng = Pcg64::seed_from_u64(seed);

        let ships: Vec<ShipState> = specs
            .into_iter()
            .map(|spec| ShipState {
                id: spec.id,
                class: spec.class,
                alive: true,
                invuln: false,
                pos: spec.anchor_pos,
                vel: Vec2::zero(),
                heading: 0.0,
                ang_vel: 0.0,
                hull: Resource::full(params.hull_max),
                shield: Resource::full(params.shield_max),
                aether: Resource::full(params.aether_max),
                sigil: None,
                cannon_cooldown: params.cannon_start_hot,
                relics_carried: 0,
                anchor_pos: spec.anchor_pos,
                persisted: PersistedIntent::default(),
            })
            .collect();

        let scores: HashMap<ShipId, f32> =
            ships.iter().map(|s| (s.id.clone(), 0.0_f32)).collect();

        Engine {
            seed,
            params,
            tick: 0,
            ships,
            scores,
            intent_log: Vec::new(),
            rng,
        }
    }

    /// The current tick count (0 before any `step` calls).
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Advance the simulation exactly one tick.
    ///
    /// For each ship: merge the supplied `Intent` into the persisted state, then
    /// apply gameplay rules.  In issue 01 (skeleton) there are no physics or
    /// combat rules — the function records intents and increments the tick.
    ///
    /// Returns a per-ship event list (empty in this skeleton; populated in
    /// later issues as combat/economy rules land).
    pub fn step(&mut self, intents: Vec<(ShipId, Intent)>) -> Vec<(ShipId, Vec<Event>)> {
        // 1. Merge incoming intents into each ship's persisted state.
        for (id, intent) in &intents {
            if let Some(ship) = self.ships.iter_mut().find(|s| s.id == *id) {
                ship.merge_intent(intent);
            }
        }

        // 2. Record the applied (persisted) intent for every ship this tick.
        //    Using the fully-resolved state ensures replays don't need to
        //    re-run the merge logic.
        let frame: IntentFrame = self
            .ships
            .iter()
            .map(|s| (s.id.clone(), s.applied_intent()))
            .collect();
        self.intent_log.push(frame);

        // 3. TODO (issue 02+): apply physics, combat, scoring, spawning, etc.
        //    The RNG (`self.rng`) is available here for any stochastic rules.
        let _ = &mut self.rng; // suppress unused warning until physics lands

        // 4. Advance tick.
        self.tick += 1;

        // 5. Return empty events per ship (populated by later issues).
        self.ships
            .iter()
            .map(|s| (s.id.clone(), Vec::<Event>::new()))
            .collect()
    }

    /// Produce the per-ship Observation a bot would see at the current tick.
    ///
    /// Shape matches PROTOCOL.md §6:
    /// - `self_view` carries full state (including Aether and Sigil).
    /// - `ships` contains *other* ships only, with Aether and Sigil hidden.
    /// - `anchors` lists every ship's Anchor.
    ///
    /// Returns `None` if `ship_id` does not belong to this Match.
    pub fn observation(&self, ship_id: &ShipId) -> Option<Observation> {
        let ship = self.ships.iter().find(|s| &s.id == ship_id)?;

        let other_ships = self
            .ships
            .iter()
            .filter(|s| s.id != *ship_id)
            .map(|s| s.to_other_view())
            .collect();

        let anchors = self
            .ships
            .iter()
            .map(|s| AnchorView {
                ship_id: s.id.clone(),
                pos: s.anchor_pos,
            })
            .collect();

        Some(Observation {
            tick: self.tick,
            max_ticks: self.params.max_ticks,
            seed: self.seed,
            arena: ArenaDims {
                width: self.params.arena_w,
                height: self.params.arena_h,
            },
            self_view: ship.to_self_view(),
            anchors,
            ships: other_ships,
            relics: vec![],
            asteroids: vec![],
            projectiles: vec![],
            singularities: vec![],
            mines: vec![],
            scores: self.scores.clone(),
            // Events are produced by `step` and will be attached by the server
            // in a future issue.  The skeleton always returns an empty list.
            events: vec![],
        })
    }

    /// Produce the full god-mode view of the world for the Viewer / recorder.
    ///
    /// Unlike `observation`, this exposes all ships with complete state
    /// (including Aether and Sigil).  Bots never receive this view.
    pub fn god_view(&self) -> GodView {
        GodView {
            tick: self.tick,
            max_ticks: self.params.max_ticks,
            seed: self.seed,
            arena: ArenaDims {
                width: self.params.arena_w,
                height: self.params.arena_h,
            },
            ships: self.ships.iter().map(|s| s.to_god_view()).collect(),
            anchors: self
                .ships
                .iter()
                .map(|s| AnchorView {
                    ship_id: s.id.clone(),
                    pos: s.anchor_pos,
                })
                .collect(),
            relics: vec![],
            asteroids: vec![],
            projectiles: vec![],
            singularities: vec![],
            mines: vec![],
            scores: self.scores.clone(),
        }
    }

    /// The applied-intent log: one `IntentFrame` per completed tick.
    /// Combined with the match `seed`, this is sufficient to replay a Match
    /// exactly once physics/rules are in place (ADR-0003).
    pub fn intent_log(&self) -> &[IntentFrame] {
        &self.intent_log
    }
}
