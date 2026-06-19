use std::collections::HashMap;
use std::f32::consts::TAU;

use rand::RngExt;
use rand::SeedableRng;
use rand_pcg::Pcg64;
use rapier2d::prelude::*;

use crate::intent::Intent;
use crate::observation::{GodShipView, GodView, Observation, OtherShipView, SelfView};
use crate::params::Params;
use crate::types::*;
// Explicit import so the engine's own Vec2 wins over rapier2d/glam's Vec2 from the glob above.
use crate::types::Vec2;

// ─── Physics world ────────────────────────────────────────────────────────────

/// All rapier2d state for one match.
///
/// Ships are `Dynamic` rigid bodies with gravity disabled and zero rapier
/// damping — we apply our own linear‑damping multiplication each tick to
/// exactly match the harness formula:
///   vel = (vel + accel) * lin_damping  →  clamped  →  pos += vel
///
/// The `dt = 1.0` integration parameter means one rapier step = one game tick.
/// No colliders are added in issue 02; they land in issue 07 (collisions).
struct PhysicsWorld {
    bodies: RigidBodySet,
    colliders: ColliderSet,
    integration_params: IntegrationParameters,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad_phase: DefaultBroadPhase,
    narrow_phase: NarrowPhase,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    ccd_solver: CCDSolver,
}

impl PhysicsWorld {
    fn new() -> Self {
        let mut integration_params = IntegrationParameters::default();
        // 1 time unit per step = 1 game tick; pos += vel * 1.0 each step.
        integration_params.dt = 1.0;
        Self {
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            integration_params,
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
        }
    }

    /// Add a ship rigid body at `pos`; return its handle for later lookup.
    fn add_ship_body(&mut self, pos: Vec2) -> RigidBodyHandle {
        let rb = RigidBodyBuilder::dynamic()
            .translation(rapier2d::math::Vector::new(pos.x, pos.y))
            .gravity_scale(0.0)
            .linear_damping(0.0)   // manual damping; see PhysicsWorld docs
            .angular_damping(0.0)
            .can_sleep(false)
            .build();
        self.bodies.insert(rb)
    }

    /// Advance the simulation one tick.
    ///
    /// Pre‑condition: all body velocities have been set by the caller to the
    /// post‑damping, post‑cap values for this tick.  rapier then integrates
    /// `pos += vel * dt` (dt = 1.0), so `pos += vel`.
    fn step(&mut self) {
        let gravity = rapier2d::math::Vector::ZERO;
        self.pipeline.step(
            gravity,
            &self.integration_params,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd_solver,
            &(),
            &(),
        );
    }
}

// ─── Persisted intent state ───────────────────────────────────────────────────

/// The concrete values that persist between ticks when a bot omits fields.
/// Sigil is one-shot and therefore not persisted here.
#[derive(Debug, Clone, Default)]
struct PersistedIntent {
    turn: f32,
    thrust: f32,
    fire: bool,
}

// ─── Internal projectile state ────────────────────────────────────────────────

/// An arcane rune-cannon projectile in flight (engine-internal).
/// Converted to `ProjectileView` on `observation` / `god_view` queries.
struct ProjectileState {
    id: String,
    pos: Vec2,
    vel: Vec2,
    owner: ShipId,
    /// Cumulative distance traveled this projectile's lifetime.
    /// Despawned once this reaches or exceeds `params.proj_range`.
    dist_traveled: f32,
}

impl ProjectileState {
    fn to_view(&self) -> ProjectileView {
        ProjectileView {
            id: self.id.clone(),
            pos: self.pos,
            vel: self.vel,
            owner: self.owner.clone(),
        }
    }
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
    /// Ticks elapsed since this ship last took damage.
    /// Used for the Shield-regen delay (`params.shield_regen_delay`).
    /// Mirrors `Ship.unhit` in harness.py; initialised to `shield_regen_delay`
    /// so that the first-ever regen check passes immediately (ship starts full).
    ticks_since_last_hit: u32,
    /// Handle into the rapier `PhysicsWorld::bodies` set.
    /// Position and velocity are the source of truth in rapier;
    /// `ship.pos` / `ship.vel` are synced back after each physics step.
    body_handle: RigidBodyHandle,
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

// ─── Internal Relic state ─────────────────────────────────────────────────────

/// A Relic lying in the Drift (engine-internal; converted to `RelicView` on query).
struct RelicState {
    id: String,
    pos: Vec2,
}

impl RelicState {
    fn to_view(&self, relic_value: f32) -> RelicView {
        RelicView {
            id: self.id.clone(),
            pos: self.pos,
            vel: Vec2::zero(), // Relics are static in issue 03; movement added later
            value: relic_value,
        }
    }
}

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
    /// Seeded RNG — used for relic spawning and Sigil assignment.
    rng: Pcg64,
    /// rapier2d physics world — position/velocity source of truth for ships.
    physics: PhysicsWorld,
    /// Relics currently lying in the Drift.
    relics: Vec<RelicState>,
    /// Monotonically-increasing counter for unique relic IDs.
    relic_id_counter: u32,
    /// Rune-cannon projectiles currently in flight.
    projectiles: Vec<ProjectileState>,
    /// Monotonically-increasing counter for unique projectile IDs.
    proj_id_counter: u32,
}

impl Engine {
    /// Construct a new engine in its initial state.
    ///
    /// Each ship starts at its `anchor_pos` with zero velocity, full Hull /
    /// Shield / Aether, no Sigil, and the cannon on its start-hot cooldown.
    ///
    /// `params.arena_w` / `params.arena_h` are used as-is; call
    /// `scale_drift(&params, n_ships)` before construction if you want
    /// Dynamic-Drift scaling.
    pub fn new(seed: u64, params: Params, specs: Vec<ShipSpec>) -> Self {
        let mut physics = PhysicsWorld::new();

        let ships: Vec<ShipState> = specs
            .into_iter()
            .map(|spec| {
                let body_handle = physics.add_ship_body(spec.anchor_pos);
                ShipState {
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
                    // Start at shield_regen_delay so regen is active immediately;
                    // ship starts with full shield so the first regen check is a no-op.
                    ticks_since_last_hit: params.shield_regen_delay,
                    body_handle,
                }
            })
            .collect();

        let scores: HashMap<ShipId, f32> =
            ships.iter().map(|s| (s.id.clone(), 0.0_f32)).collect();

        // Spawn initial Relics: max(2, relic_field_cap / 2), matching harness.py.
        let initial_relic_count = std::cmp::max(2, params.relic_field_cap / 2) as usize;
        let mut relics: Vec<RelicState> = Vec::with_capacity(initial_relic_count);
        let mut relic_id_counter: u32 = 0;

        // RNG is seeded before ship-body construction so the order is deterministic.
        let mut rng = Pcg64::seed_from_u64(seed);
        for _ in 0..initial_relic_count {
            let lo_x = 100.0_f32;
            let hi_x = (params.arena_w - 100.0).max(lo_x + f32::EPSILON);
            let lo_y = 100.0_f32;
            let hi_y = (params.arena_h - 100.0).max(lo_y + f32::EPSILON);
            let x: f32 = rng.random_range(lo_x..hi_x);
            let y: f32 = rng.random_range(lo_y..hi_y);
            relics.push(RelicState {
                id: format!("relic-{relic_id_counter}"),
                pos: Vec2::new(x, y),
            });
            relic_id_counter += 1;
        }

        Engine {
            seed,
            params,
            tick: 0,
            ships,
            scores,
            intent_log: Vec::new(),
            rng,
            physics,
            relics,
            relic_id_counter,
            projectiles: Vec::new(),
            proj_id_counter: 0,
        }
    }

    /// The current tick count (0 before any `step` calls).
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Advance the simulation exactly one tick.
    ///
    /// Per-tick sequence (matches harness.py):
    ///   1. Merge incoming intents into each ship's persisted state.
    ///   2. For each alive ship:
    ///      a. Rotate heading by `turn * max_turn`.
    ///      b. Compute thrust acceleration (zero when aether is empty).
    ///      c. Apply:  vel = (vel + accel_vec) * lin_damping
    ///      d. Clamp speed to max_speed.
    ///      e. Deduct aether cost; apply aether regen.
    ///   3. Set the computed velocity on each ship's rapier body.
    ///   4. Step rapier (moves bodies by `vel * dt`, dt = 1.0).
    ///   5. Sync positions back from rapier bodies into `ship.pos`.
    ///   5b. Relic pickup: each alive ship picks up nearby Relics up to carry_cap.
    ///   5c. Relic banking: each ship at its Anchor banks carried Relics into score.
    ///   6. Record applied intents.
    ///   7. Advance tick.
    ///   7b. Relic replenishment: spawn one Relic if tick % relic_spawn_period == 0.
    pub fn step(&mut self, intents: Vec<(ShipId, Intent)>) -> Vec<(ShipId, Vec<Event>)> {
        // 1. Merge incoming intents into each ship's persisted state.
        for (id, intent) in &intents {
            if let Some(ship) = self.ships.iter_mut().find(|s| s.id == *id) {
                ship.merge_intent(intent);
            }
        }

        // 2 & 3. Physics: compute new velocities, store them for rapier.
        //
        // We read velocity from ship.vel (kept in sync with rapier after every
        // step) to avoid a borrowing conflict between &mut self.ships and
        // &mut self.physics.bodies in the same loop body.
        let mut vel_updates: Vec<(RigidBodyHandle, f32, f32)> = Vec::new();

        for ship in &mut self.ships {
            if !ship.alive {
                vel_updates.push((ship.body_handle, 0.0, 0.0));
                continue;
            }

            let p = &self.params;
            let turn   = ship.persisted.turn;
            let thrust = ship.persisted.thrust;

            // a. Rotate heading (rate-first; clamped via max_turn).
            ship.heading = (ship.heading + turn * p.max_turn).rem_euclid(TAU);
            ship.ang_vel = turn * p.max_turn;

            // b. Thrust is ineffective at zero aether.
            let effective_thrust = if ship.aether.cur > 0.0 { thrust } else { 0.0 };

            // c. Accelerate along heading, then damp.
            let base_accel = if effective_thrust >= 0.0 {
                p.thrust_accel
            } else {
                p.reverse_accel
            };
            let accel_mag = effective_thrust * base_accel;
            let ax = ship.heading.cos() * accel_mag;
            let ay = ship.heading.sin() * accel_mag;

            let mut nvx = (ship.vel.x + ax) * p.lin_damping;
            let mut nvy = (ship.vel.y + ay) * p.lin_damping;

            // d. Cap speed.
            let spd = (nvx * nvx + nvy * nvy).sqrt();
            if spd > p.max_speed {
                let scale = p.max_speed / spd;
                nvx *= scale;
                nvy *= scale;
            }

            // Store new velocity in ship state.
            ship.vel = Vec2::new(nvx, nvy);

            // e. Aether: deduct thrust cost, then regen (clamped).
            let aether_cost = effective_thrust.abs() * p.thrust_cost_full;
            ship.aether.cur =
                (ship.aether.cur - aether_cost + p.aether_regen).clamp(0.0, ship.aether.max);

            vel_updates.push((ship.body_handle, nvx, nvy));
        }

        // 3. Push computed velocities into rapier bodies.
        for (handle, vx, vy) in &vel_updates {
            if let Some(body) = self.physics.bodies.get_mut(*handle) {
                body.set_linvel(rapier2d::math::Vector::new(*vx, *vy), true);
            }
        }

        // 4. Step rapier: pos += vel * dt (dt = 1.0).
        self.physics.step();

        // 5. Sync positions back from rapier.
        for i in 0..self.ships.len() {
            let handle = self.ships[i].body_handle;
            let (px, py) = if let Some(body) = self.physics.bodies.get(handle) {
                let t = body.translation();
                (t.x, t.y)
            } else {
                continue;
            };
            self.ships[i].pos = Vec2::new(px, py);
        }

        // 5_a. Combat: shield regen, cannon cooldown tick-down, and firing.
        //
        // Mirrors harness.py ship-loop order: regen → cd → fire.
        // Projectile spawning uses a local ID counter to avoid re-borrowing `self`.
        {
            let mut new_projectiles: Vec<ProjectileState> = Vec::new();
            let mut next_proj_id = self.proj_id_counter;

            for ship in &mut self.ships {
                if !ship.alive {
                    continue;
                }
                let p = &self.params;

                // Shield regen: increment unhit counter; regen once delay elapses.
                ship.ticks_since_last_hit = ship.ticks_since_last_hit.saturating_add(1);
                if ship.ticks_since_last_hit >= p.shield_regen_delay {
                    ship.shield.cur = (ship.shield.cur + p.shield_regen).min(ship.shield.max);
                }

                // Cannon cooldown: tick down toward 0.
                if ship.cannon_cooldown > 0 {
                    ship.cannon_cooldown -= 1;
                }

                // Fire: spawn a projectile when trigger held, cannon ready, aether available.
                if ship.persisted.fire
                    && ship.cannon_cooldown == 0
                    && ship.aether.cur >= p.shot_cost
                {
                    let vx = ship.heading.cos() * p.proj_speed;
                    let vy = ship.heading.sin() * p.proj_speed;
                    new_projectiles.push(ProjectileState {
                        id: format!("proj-{next_proj_id}"),
                        pos: ship.pos,
                        vel: Vec2::new(vx, vy),
                        owner: ship.id.clone(),
                        dist_traveled: 0.0,
                    });
                    next_proj_id += 1;
                    ship.aether.cur -= p.shot_cost;
                    ship.cannon_cooldown = p.cannon_cooldown;
                }
            }

            self.proj_id_counter = next_proj_id;
            self.projectiles.extend(new_projectiles);
        }

        // 5b. Relic pickup: for each alive ship, consume nearby Relics up to carry_cap.
        //
        // Matching harness.py per-ship order: iterate ships in order; each ship
        // processes ALL relics in the field and grabs what it can before the next
        // ship gets a turn.  `mem::take` temporarily moves relics out so we can
        // borrow `self.ships` mutably without aliasing.
        {
            let pickup_r_sq = self.params.relic_pickup_radius * self.params.relic_pickup_radius;
            let carry_cap   = self.params.carry_cap;
            let mut relics  = std::mem::take(&mut self.relics);

            for ship in &mut self.ships {
                if !ship.alive { continue; }

                // Walk the relic list; swap_remove any that this ship picks up.
                let mut i = 0;
                while i < relics.len() {
                    if ship.relics_carried >= carry_cap {
                        break;
                    }
                    let dx = relics[i].pos.x - ship.pos.x;
                    let dy = relics[i].pos.y - ship.pos.y;
                    if dx * dx + dy * dy <= pickup_r_sq {
                        ship.relics_carried += 1;
                        relics.swap_remove(i);
                        // Do NOT increment i: the swapped-in element needs checking.
                    } else {
                        i += 1;
                    }
                }
            }

            self.relics = relics;
        }

        // 5c. Relic banking: each alive ship at its Anchor banks carried Relics.
        //
        // Banking happens after pickup in the same tick, matching harness.py.
        // Score updates are deferred to avoid borrowing both ships and scores.
        {
            let bank_r_sq   = self.params.anchor_bank_radius * self.params.anchor_bank_radius;
            let relic_value = self.params.relic_value;
            let mut score_deltas: Vec<(ShipId, f32)> = Vec::new();

            for ship in &mut self.ships {
                if !ship.alive || ship.relics_carried == 0 { continue; }
                let dx = ship.pos.x - ship.anchor_pos.x;
                let dy = ship.pos.y - ship.anchor_pos.y;
                if dx * dx + dy * dy <= bank_r_sq {
                    let banked = ship.relics_carried as f32 * relic_value;
                    score_deltas.push((ship.id.clone(), banked));
                    ship.relics_carried = 0;
                }
            }

            for (id, delta) in score_deltas {
                *self.scores.entry(id).or_insert(0.0) += delta;
            }
        }

        // 5d. Projectile movement, range despawn, hit detection, and damage events.
        //
        // `mem::take` moves projectiles out of `self` so we can borrow `self.ships`
        // mutably without aliasing — the same pattern used for relics above.
        // Matching harness.py: move each projectile THEN check hits in the same tick.
        let mut ship_events: HashMap<ShipId, Vec<Event>> =
            self.ships.iter().map(|s| (s.id.clone(), Vec::new())).collect();
        // Kills this tick: (victim_id, killer_id).  Processed after the projectile
        // block so score updates and KilledShip events can access `self.scores`
        // without conflicting with the `self.ships` mutable borrow inside the block.
        let mut kills: Vec<(ShipId, ShipId)> = Vec::new();
        {
            let ship_radius_sq =
                self.params.ship_radius * self.params.ship_radius;
            let proj_range  = self.params.proj_range;
            let cannon_dmg  = self.params.cannon_damage;

            let projs = std::mem::take(&mut self.projectiles);
            let mut alive_projs: Vec<ProjectileState> = Vec::new();

            for mut proj in projs {
                // Move projectile this tick.
                proj.pos.x += proj.vel.x;
                proj.pos.y += proj.vel.y;
                let step_dist =
                    (proj.vel.x * proj.vel.x + proj.vel.y * proj.vel.y).sqrt();
                proj.dist_traveled += step_dist;

                // Range despawn: projectile has traveled its maximum distance.
                if proj.dist_traveled >= proj_range {
                    continue;
                }

                // Hit detection: first alive non-owner ship within ship_radius.
                let mut hit = false;
                for ship in &mut self.ships {
                    if !ship.alive || ship.id == proj.owner {
                        continue;
                    }
                    let dx = ship.pos.x - proj.pos.x;
                    let dy = ship.pos.y - proj.pos.y;
                    if dx * dx + dy * dy < ship_radius_sq {
                        let killed = apply_cannon_damage(
                            ship,
                            cannon_dmg,
                            &proj.owner,
                            &mut ship_events,
                        );
                        if let Some(killer_id) = killed {
                            kills.push((ship.id.clone(), killer_id));
                        }
                        hit = true;
                        break;
                    }
                }

                if !hit {
                    alive_projs.push(proj);
                }
            }

            self.projectiles = alive_projs;
        }

        // 5e. Kill resolution: award kill_bounty to each killer and emit KilledShip.
        //
        // A kill is only attributed when a rune-cannon projectile dealt the lethal
        // blow; environmental / collision deaths (issue 07+) produce `Died { by: None }`
        // with no bounty — there are no such kills to process here yet.
        //
        // Seam for issue 06: after awarding the bounty, drop the victim's relics and
        // schedule their respawn (respawn_delay / invuln fields live on ShipState).
        {
            let kill_bounty = self.params.kill_bounty;
            for (victim_id, killer_id) in kills {
                *self.scores.entry(killer_id.clone()).or_insert(0.0) += kill_bounty;
                ship_events
                    .entry(killer_id)
                    .or_default()
                    .push(Event::KilledShip { victim: victim_id });
            }
        }

        // 6. Record the applied (persisted) intent for every ship this tick.
        let frame: IntentFrame = self
            .ships
            .iter()
            .map(|s| (s.id.clone(), s.applied_intent()))
            .collect();
        self.intent_log.push(frame);

        // 7. Advance tick.
        self.tick += 1;

        // 7b. Relic replenishment: spawn one Relic every relic_spawn_period ticks,
        //     matching harness.py `if tick % spawn_period == 0`.
        if self.params.relic_spawn_period > 0
            && self.tick % self.params.relic_spawn_period == 0
        {
            self.spawn_relic_if_below_cap();
        }

        // 8. Return accumulated events per ship.
        self.ships
            .iter()
            .map(|s| (s.id.clone(), ship_events.remove(&s.id).unwrap_or_default()))
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
            relics: self
                .relics
                .iter()
                .map(|r| r.to_view(self.params.relic_value))
                .collect(),
            asteroids: vec![],
            projectiles: self.projectiles.iter().map(|p| p.to_view()).collect(),
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
            relics: self
                .relics
                .iter()
                .map(|r| r.to_view(self.params.relic_value))
                .collect(),
            asteroids: vec![],
            projectiles: self.projectiles.iter().map(|p| p.to_view()).collect(),
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

    // ─── Match result accessors ───────────────────────────────────────────────

    /// `true` once the match has reached `params.max_ticks`.
    ///
    /// The Arena Server should stop calling `step` after this returns `true`.
    pub fn is_match_over(&self) -> bool {
        self.tick >= self.params.max_ticks
    }

    /// The ship with the highest score at match end, or `None` if the match
    /// is still in progress.
    ///
    /// On a score tie the ship that appears first in the engine's ship order
    /// (i.e. the order passed to `Engine::new`) is returned — deterministic
    /// given the same construction arguments.
    pub fn winner(&self) -> Option<ShipId> {
        if !self.is_match_over() {
            return None;
        }
        // Iterate in ship construction order so ties break deterministically.
        self.ships
            .iter()
            .filter_map(|s| self.scores.get(&s.id).map(|&sc| (s.id.clone(), sc)))
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| id)
    }

    /// The current banked score for `ship_id`, or `None` if the id is unknown.
    pub fn score(&self, ship_id: &ShipId) -> Option<f32> {
        self.scores.get(ship_id).copied()
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    /// Spawn one Relic in the Drift if the field is below `relic_field_cap`.
    ///
    /// Position is uniform-random in the interior margin `[100, arena - 100]`,
    /// matching `harness.py World.spawn_relic`.  Uses the engine's seeded RNG
    /// so relic spawns are deterministic given the same seed + intent log.
    fn spawn_relic_if_below_cap(&mut self) {
        if self.relics.len() >= self.params.relic_field_cap as usize {
            return;
        }
        let lo_x = 100.0_f32;
        let hi_x = (self.params.arena_w - 100.0).max(lo_x + f32::EPSILON);
        let lo_y = 100.0_f32;
        let hi_y = (self.params.arena_h - 100.0).max(lo_y + f32::EPSILON);
        let x: f32 = self.rng.random_range(lo_x..hi_x);
        let y: f32 = self.rng.random_range(lo_y..hi_y);
        let id = format!("relic-{}", self.relic_id_counter);
        self.relic_id_counter += 1;
        self.relics.push(RelicState { id, pos: Vec2::new(x, y) });
    }
}

// ─── Dynamic Drift scaling ────────────────────────────────────────────────────

/// Scale Drift (arena) dimensions for `n_ships` ships.
///
/// Area scales proportionally to ship count off the 2000×1200 baseline for 4
/// ships, keeping density constant:
///
///   scale = √(n / 4)
///   width  = base_params.arena_w × scale
///   height = base_params.arena_h × scale
///
/// The Arena Server calls this before constructing the engine so that
/// `Observation::arena` reports the correct match-specific dimensions.
///
/// Asteroid / relic entity counts (added in later issues) also scale with N;
/// this function updates the relevant Params fields so the engine gets them all
/// from one call.
pub fn scale_drift(base: &Params, n_ships: usize) -> Params {
    let f = ((n_ships as f32) / 4.0_f32).sqrt();
    let mut p = base.clone();
    p.arena_w = base.arena_w * f;
    p.arena_h = base.arena_h * f;
    // Entity counts scale linearly with N; relic spawn period scales inversely.
    // (Asteroid/relic placement uses these in issues 03+.)
    p.n_asteroids = ((base.n_asteroids as f32) * (n_ships as f32) / 4.0).round().max(1.0) as u32;
    p.relic_field_cap =
        ((base.relic_field_cap as f32) * (n_ships as f32) / 4.0).round().max(2.0) as u32;
    p.relic_spawn_period =
        ((base.relic_spawn_period as f32) * 4.0 / (n_ships as f32)).round().max(15.0) as u32;
    p
}

// ─── Damage helper ────────────────────────────────────────────────────────────

/// Apply `damage` from a rune-cannon projectile owned by `by` to `ship`.
///
/// Damage hits the **Shield** first; any overflow goes to the **Hull**.
/// Resets `ship.ticks_since_last_hit` to 0 (pausing shield regen).
/// Marks `ship.alive = false` when Hull reaches 0 and emits `Event::Died`.
///
/// Returns `Some(killer_id)` when this hit was lethal (Hull reached 0),
/// or `None` otherwise.  The caller uses this to award the kill bounty and
/// emit `Event::KilledShip` to the killer.
///
/// Emits `Event::TookShield`, `Event::ShieldDown`, and/or `Event::TookHull`
/// into `events` for the target ship.
fn apply_cannon_damage(
    ship: &mut ShipState,
    damage: f32,
    by: &ShipId,
    events: &mut HashMap<ShipId, Vec<Event>>,
) -> Option<ShipId> {
    if !ship.alive || damage <= 0.0 {
        return None;
    }

    // Reset the shield-regen delay: the ship has been hit this tick.
    ship.ticks_since_last_hit = 0;

    // Shield absorbs as much as it can; remainder overflows to Hull.
    let shield_absorbed = damage.min(ship.shield.cur);
    let hull_overflow   = damage - shield_absorbed;

    if shield_absorbed > 0.0 {
        ship.shield.cur -= shield_absorbed;
        events
            .entry(ship.id.clone())
            .or_default()
            .push(Event::TookShield {
                amount: shield_absorbed,
                by: by.clone(),
            });
        if ship.shield.cur == 0.0 {
            events
                .entry(ship.id.clone())
                .or_default()
                .push(Event::ShieldDown);
        }
    }

    if hull_overflow > 0.0 {
        ship.hull.cur -= hull_overflow;
        // Clamp so hull never goes below 0.
        if ship.hull.cur < 0.0 {
            ship.hull.cur = 0.0;
        }
        events
            .entry(ship.id.clone())
            .or_default()
            .push(Event::TookHull {
                amount: hull_overflow,
                by: by.clone(),
            });
        if ship.hull.cur <= 0.0 {
            ship.alive = false;
            // Emit Died to the victim.
            events
                .entry(ship.id.clone())
                .or_default()
                .push(Event::Died { by: Some(by.clone()) });
            // Seam for issue 06:
            //   1. Drop carried relics at ship.pos and emit RelicDropped events.
            //   2. Schedule respawn at anchor after params.respawn_delay ticks.
            //   3. Set ship.invuln = true for params.respawn_invuln ticks post-respawn.
            return Some(by.clone());
        }
    }

    None
}
