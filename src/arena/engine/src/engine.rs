use std::collections::HashMap;
use std::f32::consts::TAU;

use rand::seq::IndexedRandom;
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

// ─── Sigil roster & effect-dispatch seam ─────────────────────────────────────

/// All five Sigil variants, in canonical order.
/// Used for random selection on Relic pickup (see relic-pickup block in `step()`).
///
/// Verified against rand 0.10.1 source:
///   ~/.cargo/registry/src/…/rand-0.10.1/src/seq/slice.rs — `IndexedRandom::choose`
///   Returns `Some(&self[rng.random_range(..self.len())])`, deterministic with a
///   seeded RNG.
const SIGILS: &[Sigil] = &[
    Sigil::Afterburner,
    Sigil::Bulwark,
    Sigil::Singularity,
    Sigil::AetherMine,
    Sigil::ArcLance,
];

/// **Effect-dispatch seam** — called immediately after the Sigil is removed
/// from `ship.sigil` during a discharge Intent.
///
/// For **self-buff** Sigils (Afterburner, Bulwark) the function mutates `ship`
/// directly and returns `SigilWorldEffect::None`.
///
/// For **world-effect** Sigils (Singularity, AetherMine, ArcLance) the function
/// returns a `SigilWorldEffect` command that `Engine::step` processes after the
/// per-ship loop, where it has full access to world state (singularities, mines,
/// rng, etc.).  The `sigil_target` from the incoming Intent is passed in as
/// `target_hint`; if absent the caller should pass `None` and the arm falls back
/// to the discharging ship's current position / heading.
///
/// ADR: World-effect dispatch uses the "return-a-command" pattern to avoid
/// borrow-checker issues (ship is mutably borrowed inside the sigil loop; we
/// can't also mutably borrow self.singularities etc. at the same time).
#[derive(Debug)]
enum SigilWorldEffect {
    None,
    DeploySingularity { owner: ShipId, pos: Vec2 },
    DropMine          { owner: ShipId, pos: Vec2 },
    FireLance         { owner: ShipId, pos: Vec2, heading: f32 },
}

fn dispatch_sigil_effect(
    sigil: &Sigil,
    ship: &mut ShipState,
    params: &Params,
    _events: &mut Vec<Event>,
    target_hint: Option<Vec2>,
) -> SigilWorldEffect {
    match sigil {
        Sigil::Afterburner => {
            // Sustained thrust/speed boost for afterburner_dur ticks.
            // "+1" compensates for the tick-down that runs later in the same
            // step, so the ship gets exactly afterburner_dur boosted physics
            // ticks starting from the step after discharge.
            ship.afterburner_ticks_left = params.afterburner_dur + 1;
            SigilWorldEffect::None
        }
        Sigil::Bulwark => {
            // Overcharge Shield to shield_max and grant damage immunity for
            // bulwark_immunity ticks (reuses the existing invuln mechanism so
            // both cannon and collision damage are blocked by the same guard).
            //
            // "+1" compensates for the tick-down that runs later in the same
            // step as the discharge (5_sigil runs before the tick-down), giving
            // the ship exactly bulwark_immunity protected ticks starting from
            // the discharge step onwards.
            ship.shield.cur = params.shield_max;
            ship.invuln = true;
            ship.invuln_ticks_left = params.bulwark_immunity + 1;
            ship.bulwark_ticks_left = params.bulwark_immunity + 1;
            SigilWorldEffect::None
        }
        Sigil::Singularity => {
            // Deploy gravity well at sigil_target (or ship position if no target).
            let pos = target_hint.unwrap_or(ship.pos);
            SigilWorldEffect::DeploySingularity { owner: ship.id.clone(), pos }
        }
        Sigil::AetherMine => {
            // Drop a proximity mine at the ship's current position.
            SigilWorldEffect::DropMine { owner: ship.id.clone(), pos: ship.pos }
        }
        Sigil::ArcLance => {
            // Fire a piercing bolt toward sigil_target (or along current heading).
            let heading = if let Some(t) = target_hint {
                let dx = t.x - ship.pos.x;
                let dy = t.y - ship.pos.y;
                if dx.abs() > 1e-6 || dy.abs() > 1e-6 {
                    dy.atan2(dx)
                } else {
                    ship.heading
                }
            } else {
                ship.heading
            };
            SigilWorldEffect::FireLance {
                owner: ship.id.clone(),
                pos: ship.pos,
                heading,
            }
        }
    }
}

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

// ─── Internal Singularity state ───────────────────────────────────────────────

/// A Singularity gravity well deployed in the Drift (engine-internal).
/// Converted to `SingularityView` on `observation` / `god_view` queries.
struct SingularityState {
    id: String,
    pos: Vec2,
    /// Owner ship — the singularity is visible to everyone but its origin is tracked
    /// for future anti-grief rules.
    #[allow(dead_code)]
    owner: ShipId,
    /// Remaining ticks before the well collapses (expires when reaches 0).
    ticks_left: u32,
}

impl SingularityState {
    fn to_view(&self, radius: f32) -> SingularityView {
        SingularityView {
            id: self.id.clone(),
            pos: self.pos,
            radius,
            ticks_left: self.ticks_left,
        }
    }
}

// ─── Internal Mine state ──────────────────────────────────────────────────────

/// An Aether Mine lying in the Drift (engine-internal).
/// Converted to `MineView` on `observation` / `god_view` queries.
/// Visibility rules (PROTOCOL §6):
///   - Owner always sees their own mine (`own = true`).
///   - Enemy ships see it only when within `mine_radius` (proximity-visible).
struct MineState {
    id: String,
    pos: Vec2,
    owner: ShipId,
    /// Ticks until the mine is armed and can detonate.
    /// Counts down from `params.mine_arm`; 0 means armed and ready.
    arm_ticks_left: u32,
}

impl MineState {
    fn to_view(&self, own: bool) -> MineView {
        MineView {
            id: self.id.clone(),
            pos: self.pos,
            own,
        }
    }
}

// ─── Internal Arc Lance bolt state ────────────────────────────────────────────

/// A fast, piercing Arc Lance bolt in flight (engine-internal).
/// Unlike rune-cannon projectiles, lance bolts:
///   - Travel at `lance_speed` per tick.
///   - Bypass Shield: deal `lance_damage` directly to Hull.
///   - Pierce: do NOT stop on first hit; continue until range is exhausted.
struct LanceBoltState {
    #[allow(dead_code)]
    id: String,
    pos: Vec2,
    vel: Vec2,
    owner: ShipId,
    /// Cumulative distance traveled; bolt despawns at `params.proj_range`.
    dist_traveled: f32,
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
    /// Ticks remaining before this dead ship respawns at its Anchor.
    /// Set to `params.respawn_delay` on death; counted down each tick.
    /// 0 means the ship is alive (or not yet scheduled).
    respawn_ticks_left: u32,
    /// Ticks remaining for spawn-protection invulnerability after respawn.
    /// Counted down each tick while > 0; when it reaches 0 `invuln` is cleared.
    /// A value of 0 with `invuln = true` means the flag was set externally
    /// (e.g. `set_invuln_for_test`) and persists indefinitely.
    invuln_ticks_left: u32,
    /// Ticks remaining for the Afterburner boost window.
    /// Set to `params.afterburner_dur + 1` on discharge; decremented each tick.
    /// 0 means inactive.  When it reaches 0, `AfterburnerExpired` is emitted.
    afterburner_ticks_left: u32,
    /// Ticks remaining for the Bulwark immunity window (mirrors `invuln_ticks_left`
    /// when set by Bulwark; decremented in sync).  0 means no Bulwark active.
    /// When it reaches 0, `BulwarkExpired` is emitted.
    bulwark_ticks_left: u32,
    /// Sigil discharge recorded during step 5_sigil this tick, for inclusion in
    /// the intent log.  `Some(target)` means the ship discharged its Sigil this
    /// tick (with the given `sigil_target` from the intent, which may be `None`
    /// for untargeted Sigils).  Cleared when the intent frame is recorded in
    /// step 6 so that stale values never carry over into the next tick.
    ///
    /// Rationale: `sigil` is one-shot and not persisted, so `applied_intent()`
    /// cannot see it by the time step 6 runs — the Sigil has already been taken
    /// from `ship.sigil` in step 5_sigil.  This field bridges that gap so the
    /// intent log captures every discharge for exact replay parity.
    sigil_discharge_this_tick: Option<Option<Vec2>>,
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
    ///
    /// `sigil_discharge`: pass `Some(target_hint)` when this ship discharged its
    /// Sigil this tick (recorded by step 5_sigil into `sigil_discharge_this_tick`).
    /// The discharge is one-shot and not persisted, so it must be supplied here.
    fn applied_intent(&self, sigil_discharge: Option<Option<Vec2>>) -> Intent {
        Intent {
            turn: Some(self.persisted.turn),
            thrust: Some(self.persisted.thrust),
            fire: Some(self.persisted.fire),
            sigil: sigil_discharge.as_ref().map(|_| true),
            sigil_target: sigil_discharge.flatten(),
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
            afterburner_ticks_left: self.afterburner_ticks_left,
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
            afterburner_ticks_left: self.afterburner_ticks_left,
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

// ─── Internal Asteroid state ──────────────────────────────────────────────────

/// A drifting Asteroid in the Drift (engine-internal).
/// Converted to `AsteroidView` on `god_view` / `observation` queries.
/// Asteroids are collision hazards: they deal `k_asteroid`-scaled damage on contact.
struct AsteroidState {
    id: String,
    pos: Vec2,
    vel: Vec2,
    radius: f32,
}

impl AsteroidState {
    fn to_view(&self) -> AsteroidView {
        AsteroidView {
            id: self.id.clone(),
            pos: self.pos,
            vel: self.vel,
            radius: self.radius,
        }
    }
}

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
    /// Asteroids currently drifting in the Drift (collision hazards + cover).
    asteroids: Vec<AsteroidState>,
    /// Monotonically-increasing counter for unique asteroid IDs.
    #[allow(dead_code)] // reserved for future asteroid respawning
    asteroid_id_counter: u32,
    // ── Issue 10: World-effect Sigil state ─────────────────────────────────────
    /// Active Singularity gravity wells.
    singularities: Vec<SingularityState>,
    /// Monotonically-increasing counter for unique singularity IDs.
    singularity_id_counter: u32,
    /// Aether Mines in the Drift (armed or arming).
    mines: Vec<MineState>,
    /// Monotonically-increasing counter for unique mine IDs.
    mine_id_counter: u32,
    /// Arc Lance bolts in flight (pierce-capable, shield-bypassing).
    lance_bolts: Vec<LanceBoltState>,
    /// Monotonically-increasing counter for unique lance bolt IDs.
    lance_id_counter: u32,
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
                    respawn_ticks_left: 0,
                    invuln_ticks_left: 0,
                    afterburner_ticks_left: 0,
                    bulwark_ticks_left: 0,
                    sigil_discharge_this_tick: None,
                }
            })
            .collect();

        let scores: HashMap<ShipId, f32> =
            ships.iter().map(|s| (s.id.clone(), 0.0_f32)).collect();

        // Spawn initial Relics: max(2, relic_field_cap / 2), matching harness.py.
        // Skip entirely when relic_field_cap == 0 (test isolation).
        let initial_relic_count = if params.relic_field_cap == 0 {
            0
        } else {
            std::cmp::max(2, params.relic_field_cap / 2) as usize
        };
        let mut relics: Vec<RelicState> = Vec::with_capacity(initial_relic_count);
        let mut relic_id_counter: u32 = 0;

        // RNG is seeded before ship-body construction so the order is deterministic.
        let mut rng = Pcg64::seed_from_u64(seed);
        for _ in 0..initial_relic_count {
            let lo_x = 100.0_f32;
            let hi_x = (params.arena_w - 100.0).max(lo_x + 1.0);
            let lo_y = 100.0_f32;
            let hi_y = (params.arena_h - 100.0).max(lo_y + 1.0);
            let x: f32 = rng.random_range(lo_x..hi_x);
            let y: f32 = rng.random_range(lo_y..hi_y);
            relics.push(RelicState {
                id: format!("relic-{relic_id_counter}"),
                pos: Vec2::new(x, y),
            });
            relic_id_counter += 1;
        }

        // Spawn asteroids using the engine seeded RNG (after relics, so relic
        // positions are unaffected).  Mirrors harness.py World.__init__ asteroid
        // loop.  Positions use a `ship_radius + asteroid_radius_max` safety margin
        // from each edge so that asteroids cannot overlap the arena corners where
        // many test ships spawn; velocity in [-drift, +drift].
        let n_ast = params.n_asteroids as usize;
        let mut asteroids: Vec<AsteroidState> = Vec::with_capacity(n_ast);
        let mut asteroid_id_counter: u32 = 0;
        let ast_margin = params.ship_radius + params.asteroid_radius_max;
        for _ in 0..n_ast {
            // Guard against degenerate arenas: hi must be strictly > lo.
            let lo_ax = ast_margin;
            let hi_ax = (params.arena_w - ast_margin).max(lo_ax + 1.0);
            let lo_ay = ast_margin;
            let hi_ay = (params.arena_h - ast_margin).max(lo_ay + 1.0);
            let x: f32 = rng.random_range(lo_ax..hi_ax);
            let y: f32 = rng.random_range(lo_ay..hi_ay);
            let radius: f32 = rng
                .random_range(params.asteroid_radius_min..params.asteroid_radius_max);
            let vx: f32 = rng.random_range(-params.asteroid_drift..=params.asteroid_drift);
            let vy: f32 = rng.random_range(-params.asteroid_drift..=params.asteroid_drift);
            asteroids.push(AsteroidState {
                id: format!("asteroid-{asteroid_id_counter}"),
                pos: Vec2::new(x, y),
                vel: Vec2::new(vx, vy),
                radius,
            });
            asteroid_id_counter += 1;
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
            asteroids,
            asteroid_id_counter,
            singularities: Vec::new(),
            singularity_id_counter: 0,
            mines: Vec::new(),
            mine_id_counter: 0,
            lance_bolts: Vec::new(),
            lance_id_counter: 0,
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
        // ── Issue 06: respawn tick-down ───────────────────────────────────────
        //
        // For each dead ship with a pending respawn timer, decrement the counter.
        // When it reaches 0, restore the ship to full health at its Anchor and
        // grant spawn-protection invulnerability for `respawn_invuln` ticks.
        //
        // Ordering matters: invuln tick-down runs AFTER respawn so that a
        // newly-respawned ship starts its invuln window with the full count
        // intact (not decremented in the same step it was set).
        let mut just_respawned: Vec<ShipId> = Vec::new();
        {
            let hull_max         = self.params.hull_max;
            let shield_max       = self.params.shield_max;
            let aether_max       = self.params.aether_max;
            let respawn_invuln   = self.params.respawn_invuln;
            let cannon_start_hot = self.params.cannon_start_hot;
            let shield_regen_dly = self.params.shield_regen_delay;

            let mut respawn_bodies: Vec<(RigidBodyHandle, Vec2)> = Vec::new();
            for ship in &mut self.ships {
                if ship.alive || ship.respawn_ticks_left == 0 {
                    continue;
                }
                ship.respawn_ticks_left -= 1;
                if ship.respawn_ticks_left == 0 {
                    ship.alive               = true;
                    ship.pos                 = ship.anchor_pos;
                    ship.vel                 = Vec2::zero();
                    ship.ang_vel             = 0.0;
                    ship.heading             = 0.0;
                    ship.hull                = Resource::full(hull_max);
                    ship.shield              = Resource::full(shield_max);
                    ship.aether              = Resource::full(aether_max);
                    ship.sigil               = None;
                    ship.invuln              = true;
                    // "+1" so that after the countdown tick that runs later in
                    // THIS same step the counter is exactly `respawn_invuln`,
                    // giving the full configured quota of blocked fire steps.
                    ship.invuln_ticks_left   = respawn_invuln + 1;
                    ship.cannon_cooldown     = cannon_start_hot;
                    ship.ticks_since_last_hit = shield_regen_dly;
                    ship.persisted           = PersistedIntent::default();
                    // Clear any active sigil buffs on respawn.
                    ship.afterburner_ticks_left = 0;
                    ship.bulwark_ticks_left     = 0;
                    just_respawned.push(ship.id.clone());
                    respawn_bodies.push((ship.body_handle, ship.anchor_pos));
                }
            }
            // Sync rapier bodies for respawned ships so physics starts from the
            // correct anchor position next tick.
            for (handle, anchor) in respawn_bodies {
                if let Some(body) = self.physics.bodies.get_mut(handle) {
                    body.set_translation(
                        rapier2d::math::Vector::new(anchor.x, anchor.y),
                        true,
                    );
                    body.set_linvel(rapier2d::math::Vector::new(0.0, 0.0), true);
                }
            }
        }

        // ── Issue 06: invuln tick-down ────────────────────────────────────────
        //
        // Count down spawn-protection (and future Bulwark immunity) for alive
        // invuln ships.  `invuln_ticks_left = 0` with `invuln = true` means the
        // flag was set externally (e.g. `set_invuln_for_test`) and never expires.
        //
        // NOTE: This block was deliberately removed from the START of step and
        // moved to AFTER combat (see below) so that:
        //   a) Ships respawned in this tick get their full respawn_invuln quota
        //      of blocked fire steps — the countdown only consumes one tick
        //      during the respawn step itself (before combat runs), not before.
        //   b) The "+1" on respawn_ticks_left compensates for the one tick that
        //      the countdown consumes during the respawn step.

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

            // b. Afterburner: boost thrust_accel and max_speed; bypass aether gate.
            //    While active, thrust is always effective (no aether check) and
            //    no aether is deducted.
            let ab_active = ship.afterburner_ticks_left > 0;
            let effective_thrust_accel = if ab_active {
                p.thrust_accel * p.afterburner_thrust_mult
            } else {
                p.thrust_accel
            };
            let effective_max_speed = if ab_active {
                p.max_speed * p.afterburner_speed_mult
            } else {
                p.max_speed
            };

            // b. Thrust is ineffective at zero aether — unless Afterburner is active.
            let effective_thrust = if ab_active || ship.aether.cur > 0.0 { thrust } else { 0.0 };

            // c. Accelerate along heading, then damp.
            let base_accel = if effective_thrust >= 0.0 {
                effective_thrust_accel
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
            if spd > effective_max_speed {
                let scale = effective_max_speed / spd;
                nvx *= scale;
                nvy *= scale;
            }

            // Store new velocity in ship state.
            ship.vel = Vec2::new(nvx, nvy);

            // e. Aether: deduct thrust cost (free while Afterburner active), then regen.
            let aether_cost = if ab_active {
                0.0
            } else {
                effective_thrust.abs() * p.thrust_cost_full
            };
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

        // Initialise per-ship event queues early so collision blocks can push into them.
        let mut ship_events: HashMap<ShipId, Vec<Event>> =
            self.ships.iter().map(|s| (s.id.clone(), Vec::new())).collect();

        // Emit Respawned event to each ship that came back to life this tick.
        for id in just_respawned {
            ship_events.entry(id).or_default().push(Event::Respawned);
        }

        // 5_coll_0. Asteroid drift (always applies, collision_enabled or not).
        //
        // Asteroids move by their constant drift velocity each tick and wrap
        // around the arena edges, matching harness.py:
        //   a[0] = (a[0] + a[3]) % arena_w
        //   a[1] = (a[1] + a[4]) % arena_h
        let aw = self.params.arena_w;
        let ah = self.params.arena_h;
        for asteroid in &mut self.asteroids {
            asteroid.pos.x = (asteroid.pos.x + asteroid.vel.x).rem_euclid(aw);
            asteroid.pos.y = (asteroid.pos.y + asteroid.vel.y).rem_euclid(ah);
        }

        if self.params.collision_enabled {
        // Collect env deaths (id, death-pos, relics-carried) so we can drop
        // their relics and schedule respawn after the collision loops end.
        // We clear ship.relics_carried inline to avoid double-drops.
        let respawn_delay_env = self.params.respawn_delay;
        let mut env_death_drops: Vec<(ShipId, Vec2, u32)> = Vec::new();

        // 5_coll_1. Wall collision detection & response.
        //
        // Only triggered when the ship is actively moving INTO the wall
        // (velocity component toward the wall is non-zero).  This prevents
        // teleporting ships that happen to start near a wall edge.
        //
        // Mirrors harness.py: impact_speed = abs(velocity_component);
        // bounce: velocity_component *= -0.5 (50 % elastic);
        // damage = max(0, impact_speed − threshold) × k_wall.
        {
            let p = &self.params;
            let ship_r   = p.ship_radius;
            let threshold = p.coll_threshold;
            let k_wall   = p.k_wall;
            let arena_w  = p.arena_w;
            let arena_h  = p.arena_h;

            for ship in &mut self.ships {
                if !ship.alive { continue; }

                // Left wall (x = 0): ship penetrates when pos.x < ship_r.
                if ship.pos.x < ship_r && ship.vel.x < 0.0 {
                    ship.pos.x = ship_r;
                    let impact = ship.vel.x.abs();
                    let damage = (impact - threshold).max(0.0) * k_wall;
                    if apply_env_damage(ship, damage, &mut ship_events) {
                        env_death_drops.push((ship.id.clone(), ship.pos, ship.relics_carried));
                        ship.relics_carried     = 0;
                        ship.respawn_ticks_left = respawn_delay_env;
                    }
                    ship.vel.x = -ship.vel.x * 0.5;
                }
                // Right wall (x = arena_w).
                if ship.pos.x > arena_w - ship_r && ship.vel.x > 0.0 {
                    ship.pos.x = arena_w - ship_r;
                    let impact = ship.vel.x.abs();
                    let damage = (impact - threshold).max(0.0) * k_wall;
                    if apply_env_damage(ship, damage, &mut ship_events) {
                        env_death_drops.push((ship.id.clone(), ship.pos, ship.relics_carried));
                        ship.relics_carried     = 0;
                        ship.respawn_ticks_left = respawn_delay_env;
                    }
                    ship.vel.x = -ship.vel.x * 0.5;
                }
                // Top wall (y = 0, y increases downward per PROTOCOL §3).
                if ship.pos.y < ship_r && ship.vel.y < 0.0 {
                    ship.pos.y = ship_r;
                    let impact = ship.vel.y.abs();
                    let damage = (impact - threshold).max(0.0) * k_wall;
                    if apply_env_damage(ship, damage, &mut ship_events) {
                        env_death_drops.push((ship.id.clone(), ship.pos, ship.relics_carried));
                        ship.relics_carried     = 0;
                        ship.respawn_ticks_left = respawn_delay_env;
                    }
                    ship.vel.y = -ship.vel.y * 0.5;
                }
                // Bottom wall (y = arena_h).
                if ship.pos.y > arena_h - ship_r && ship.vel.y > 0.0 {
                    ship.pos.y = arena_h - ship_r;
                    let impact = ship.vel.y.abs();
                    let damage = (impact - threshold).max(0.0) * k_wall;
                    if apply_env_damage(ship, damage, &mut ship_events) {
                        env_death_drops.push((ship.id.clone(), ship.pos, ship.relics_carried));
                        ship.relics_carried     = 0;
                        ship.respawn_ticks_left = respawn_delay_env;
                    }
                    ship.vel.y = -ship.vel.y * 0.5;
                }
            }
        }

        // 5_coll_2. Ship–asteroid collision detection & response.
        //
        // For each (ship, asteroid) pair within collision distance, only
        // process if the ship is moving TOWARD the asteroid (vn < 0, where vn
        // is the dot-product of ship velocity with the normal pointing from the
        // asteroid toward the ship).  This avoids spurious bounces for ships
        // that start inside an asteroid with zero velocity.
        //
        // Mirrors harness.py: elastic reflection (v −= 2 × vn × n).
        {
            let p = &self.params;
            let ship_r   = p.ship_radius;
            let threshold = p.coll_threshold;
            let k_ast    = p.k_asteroid;

            // Read asteroid data first to separate borrows: `&self.asteroids` vs
            // `&mut self.ships` are different fields, so Rust allows simultaneous access.
            for ship in &mut self.ships {
                if !ship.alive { continue; }
                for asteroid in &self.asteroids {
                    let dx = ship.pos.x - asteroid.pos.x;
                    let dy = ship.pos.y - asteroid.pos.y;
                    let dist_sq = dx * dx + dy * dy;
                    let rr = ship_r + asteroid.radius;
                    if dist_sq >= rr * rr || dist_sq == 0.0 {
                        continue;
                    }
                    let d = dist_sq.sqrt();
                    let nx = dx / d; // normal from asteroid → ship
                    let ny = dy / d;
                    // vn < 0 means ship velocity has a component toward the asteroid.
                    let vn = ship.vel.x * nx + ship.vel.y * ny;
                    if vn >= 0.0 {
                        continue; // separating — no collision response
                    }
                    // Snap ship to asteroid surface.
                    ship.pos.x = asteroid.pos.x + nx * rr;
                    ship.pos.y = asteroid.pos.y + ny * rr;
                    // Damage: impact speed = |vn|.
                    let damage = (vn.abs() - threshold).max(0.0) * k_ast;
                    if apply_env_damage(ship, damage, &mut ship_events) {
                        env_death_drops.push((ship.id.clone(), ship.pos, ship.relics_carried));
                        ship.relics_carried     = 0;
                        ship.respawn_ticks_left = respawn_delay_env;
                    }
                    // Elastic velocity reflection about the normal axis.
                    ship.vel.x -= 2.0 * vn * nx;
                    ship.vel.y -= 2.0 * vn * ny;
                }
            }
        }

        // 5_coll_3. Ship–ship (ram) collision detection & response.
        //
        // Each pair of alive ships is checked once.  Collision only processed
        // when the closing velocity is positive (ships approaching).
        //
        // Mirrors harness.py: relative velocity exchanged along collision normal;
        // both ships take the same k_ram-scaled damage.
        {
            let p = &self.params;
            let two_r    = 2.0 * p.ship_radius;
            let threshold = p.coll_threshold;
            let k_ram    = p.k_ram;
            let n        = self.ships.len();

            // Collect collisions first to avoid dual mutable borrows.
            struct RamEvent { i: usize, j: usize, closing: f32, nx: f32, ny: f32, damage: f32 }
            let mut ram_events: Vec<RamEvent> = Vec::new();

            for i in 0..n {
                for j in (i + 1)..n {
                    if !self.ships[i].alive || !self.ships[j].alive { continue; }
                    let dx = self.ships[j].pos.x - self.ships[i].pos.x;
                    let dy = self.ships[j].pos.y - self.ships[i].pos.y;
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq >= two_r * two_r || dist_sq == 0.0 { continue; }
                    let d = dist_sq.sqrt();
                    let nx = dx / d; // normal from ship_i → ship_j
                    let ny = dy / d;
                    // Closing velocity: relative velocity of i toward j.
                    let closing = (self.ships[i].vel.x - self.ships[j].vel.x) * nx
                                + (self.ships[i].vel.y - self.ships[j].vel.y) * ny;
                    if closing <= 0.0 { continue; } // separating
                    let damage = (closing - threshold).max(0.0) * k_ram;
                    ram_events.push(RamEvent { i, j, closing, nx, ny, damage });
                }
            }

            for ev in ram_events {
                // split_at_mut gives us two non-overlapping mutable slices.
                let (left, right) = self.ships.split_at_mut(ev.j);
                let ship_i = &mut left[ev.i];
                let ship_j = &mut right[0];

                if apply_env_damage(ship_i, ev.damage, &mut ship_events) {
                    env_death_drops.push((ship_i.id.clone(), ship_i.pos, ship_i.relics_carried));
                    ship_i.relics_carried     = 0;
                    ship_i.respawn_ticks_left = respawn_delay_env;
                }
                if apply_env_damage(ship_j, ev.damage, &mut ship_events) {
                    env_death_drops.push((ship_j.id.clone(), ship_j.pos, ship_j.relics_carried));
                    ship_j.relics_carried     = 0;
                    ship_j.respawn_ticks_left = respawn_delay_env;
                }

                // Exchange the relative velocity component along the collision normal.
                ship_i.vel.x -= ev.closing * ev.nx;
                ship_i.vel.y -= ev.closing * ev.ny;
                ship_j.vel.x += ev.closing * ev.nx;
                ship_j.vel.y += ev.closing * ev.ny;
            }
        }

        // 5_coll_4. Sync collision-corrected positions and velocities back into rapier.
        //
        // Wall/asteroid/ram responses may have changed ship.pos and ship.vel;
        // rapier must start the next tick from the corrected values.
        for ship in &mut self.ships {
            if let Some(body) = self.physics.bodies.get_mut(ship.body_handle) {
                body.set_translation(
                    rapier2d::math::Vector::new(ship.pos.x, ship.pos.y),
                    true,
                );
                body.set_linvel(
                    rapier2d::math::Vector::new(ship.vel.x, ship.vel.y),
                    true,
                );
            }
        }

        // Issue 06: process env deaths — drop carried relics into the Drift and
        // emit RelicDropped events.  Runs after all collision processing so that
        // `self.relics` (not borrowed during collision loops) is freely accessible.
        {
            let scatter_env = self.params.ship_radius * 1.5;
            for (id, pos, count) in env_death_drops {
                for _ in 0..count {
                    let angle: f32 = self.rng.random_range(0.0..TAU);
                    let r: f32     = self.rng.random_range(0.0..scatter_env);
                    let relic_pos  = Vec2::new(
                        pos.x + angle.cos() * r,
                        pos.y + angle.sin() * r,
                    );
                    let relic_id = format!("relic-{}", self.relic_id_counter);
                    self.relic_id_counter += 1;
                    self.relics.push(RelicState { id: relic_id.clone(), pos: relic_pos });
                    ship_events
                        .entry(id.clone())
                        .or_default()
                        .push(Event::RelicDropped { relic_id, pos: relic_pos });
                }
            }
        }

        } // end if self.params.collision_enabled

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

        // 5_sigil. Sigil discharge: process `intent.sigil = true` for each alive ship.
        //
        // Ordered before relic pickup (5b) so that a ship that discharges its current
        // Sigil on the same tick it picks up a relic will end the tick with the newly
        // granted Sigil (not the one it just used).
        //
        // Sigil is one-shot — not persisted in `PersistedIntent`; read directly from
        // the raw incoming intents map.
        //
        // World-effect commands (Singularity / AetherMine / ArcLance) are returned
        // as SigilWorldEffect values and executed AFTER the per-ship loop so we can
        // mutably borrow self.singularities / self.mines / self.lance_bolts without
        // conflicting with the ship borrow above.
        {
            let mut world_effects: Vec<SigilWorldEffect> = Vec::new();

            for (id, intent) in &intents {
                if intent.sigil != Some(true) {
                    continue;
                }
                if let Some(ship) = self.ships.iter_mut().find(|s| s.id == *id) {
                    if !ship.alive {
                        continue;
                    }
                    if let Some(sigil) = ship.sigil.take() {
                        let ship_ev_vec = ship_events
                            .entry(ship.id.clone())
                            .or_default();
                        let target_hint = intent.sigil_target;
                        let world_cmd = dispatch_sigil_effect(
                            &sigil,
                            ship,
                            &self.params,
                            ship_ev_vec,
                            target_hint,
                        );
                        ship_events
                            .entry(ship.id.clone())
                            .or_default()
                            .push(Event::SigilDischarged { which: sigil });
                        world_effects.push(world_cmd);
                        // Record the discharge for the intent log (step 6).
                        // Sigil is one-shot and already taken above, so we must
                        // capture the target_hint here while it is still in scope.
                        ship.sigil_discharge_this_tick = Some(target_hint);
                    }
                    // With none held: no-op (no event, no state change).
                }
            }

            // Process world-effect commands now that ship borrows are released.
            for cmd in world_effects {
                match cmd {
                    SigilWorldEffect::None => {}
                    SigilWorldEffect::DeploySingularity { owner, pos } => {
                        let id = format!("sing-{}", self.singularity_id_counter);
                        self.singularity_id_counter += 1;
                        ship_events
                            .entry(owner.clone())
                            .or_default()
                            .push(Event::SingularityDeployed { id: id.clone(), pos });
                        self.singularities.push(SingularityState {
                            id,
                            pos,
                            owner,
                            // "+1" compensates for the tick-down that runs later in the
                            // same step, so the well lasts exactly `singularity_dur`
                            // MORE steps (total pull count = singularity_dur + 1 including
                            // the discharge step, matching the pattern of Afterburner/Bulwark).
                            ticks_left: self.params.singularity_dur + 1,
                        });
                    }
                    SigilWorldEffect::DropMine { owner, pos } => {
                        let id = format!("mine-{}", self.mine_id_counter);
                        self.mine_id_counter += 1;
                        ship_events
                            .entry(owner.clone())
                            .or_default()
                            .push(Event::MineDeployed { id: id.clone(), pos });
                        self.mines.push(MineState {
                            id,
                            pos,
                            owner,
                            arm_ticks_left: self.params.mine_arm,
                        });
                    }
                    SigilWorldEffect::FireLance { owner, pos, heading } => {
                        let id = format!("lance-{}", self.lance_id_counter);
                        self.lance_id_counter += 1;
                        let vx = heading.cos() * self.params.lance_speed;
                        let vy = heading.sin() * self.params.lance_speed;
                        self.lance_bolts.push(LanceBoltState {
                            id,
                            pos,
                            vel: Vec2::new(vx, vy),
                            owner,
                            dist_traveled: 0.0,
                        });
                    }
                }
            }
        }

        // 5b. Relic pickup: for each alive ship, consume nearby Relics up to carry_cap.
        //
        // Matching harness.py per-ship order: iterate ships in order; each ship
        // processes ALL relics in the field and grabs what it can before the next
        // ship gets a turn.  `mem::take` temporarily moves relics out so we can
        // borrow `self.ships` mutably without aliasing.
        //
        // Two-pass Sigil grant: the inner loop collects ship indices that need a
        // Sigil (holding none when they pick up a relic); after the loop `self.rng`
        // is used to grant one Sigil per qualifying ship.  The two-pass approach
        // keeps `self.ships` and `self.rng` borrows non-overlapping.
        {
            let pickup_r_sq = self.params.relic_pickup_radius * self.params.relic_pickup_radius;
            let carry_cap   = self.params.carry_cap;
            let enable_sigils = self.params.enable_sigils;
            let mut relics  = std::mem::take(&mut self.relics);

            // Indices (into `self.ships`) of ships that need a Sigil grant this tick.
            let mut needs_sigil: Vec<usize> = Vec::new();

            for (ship_idx, ship) in self.ships.iter_mut().enumerate() {
                if !ship.alive { continue; }

                // Track whether this ship already qualified for a Sigil this loop.
                // (A ship picks up at most one Sigil per tick even if it grabs
                // multiple Relics in the same batch.)
                let mut this_ship_queued = false;

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

                        // Sigil grant: first relic pickup while holding none.
                        if enable_sigils && ship.sigil.is_none() && !this_ship_queued {
                            needs_sigil.push(ship_idx);
                            this_ship_queued = true;
                        }
                    } else {
                        i += 1;
                    }
                }
            }

            self.relics = relics;

            // Pass 2: grant Sigils outside the ships loop so `self.rng` can be
            // borrowed independently of `self.ships`.
            // `IndexedRandom::choose` on a non-empty slice always returns `Some`.
            for idx in needs_sigil {
                // Re-check: the discharge block above may have cleared sigil (if the
                // ship discharged and then picked up a relic in the same tick —
                // sigil was taken in 5_sigil, so sigil is already None here → grant).
                if self.ships[idx].sigil.is_none() {
                    // SIGILS is a non-empty static slice; unwrap is safe.
                    let granted = SIGILS.choose(&mut self.rng).unwrap().clone();
                    let ship_id = self.ships[idx].id.clone();
                    self.ships[idx].sigil = Some(granted.clone());
                    ship_events
                        .entry(ship_id)
                        .or_default()
                        .push(Event::SigilGranted { which: granted });
                }
            }
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
        // Issue 06: after awarding the bounty, drop the victim's carried relics back
        // into the Drift (with small RNG scatter for visual spread) and schedule their
        // respawn.  Scatter uses the engine's seeded RNG so the drop is deterministic.
        {
            let kill_bounty    = self.params.kill_bounty;
            let respawn_delay  = self.params.respawn_delay;
            let scatter_radius = self.params.ship_radius * 1.5;

            for (victim_id, killer_id) in kills {
                *self.scores.entry(killer_id.clone()).or_insert(0.0) += kill_bounty;
                ship_events
                    .entry(killer_id)
                    .or_default()
                    .push(Event::KilledShip { victim: victim_id.clone() });

                // Collect drop info and mutate victim fields while holding the
                // mutable borrow; release it before pushing to self.relics.
                let (drop_pos, drop_count) = {
                    let victim = self.ships.iter_mut().find(|s| s.id == victim_id);
                    if let Some(v) = victim {
                        let pos   = v.pos;
                        let count = v.relics_carried;
                        v.relics_carried       = 0;
                        v.respawn_ticks_left   = respawn_delay;
                        (pos, count)
                    } else {
                        continue;
                    }
                };

                // Drop each carried relic back into the Drift with a small scatter.
                for _ in 0..drop_count {
                    let angle: f32 = self.rng.random_range(0.0..TAU);
                    let r: f32     = self.rng.random_range(0.0..scatter_radius);
                    let relic_pos  = Vec2::new(
                        drop_pos.x + angle.cos() * r,
                        drop_pos.y + angle.sin() * r,
                    );
                    let relic_id = format!("relic-{}", self.relic_id_counter);
                    self.relic_id_counter += 1;
                    self.relics.push(RelicState { id: relic_id.clone(), pos: relic_pos });
                    ship_events
                        .entry(victim_id.clone())
                        .or_default()
                        .push(Event::RelicDropped { relic_id, pos: relic_pos });
                }
            }
        }

        // ── Issue 10: Singularity tick-down and gravitational pull ───────────
        //
        // Each active singularity:
        //   1. Pulls each alive non-owner enemy ship within singularity_radius
        //      toward the well center by singularity_pull units/tick (velocity impulse).
        //   2. Pulls each loose Relic within singularity_radius toward the center
        //      by singularity_pull units/tick (direct position adjustment).
        //   3. Decrements ticks_left; removes the well when it reaches 0.
        {
            let sing_radius  = self.params.singularity_radius;
            let sing_pull    = self.params.singularity_pull;

            // Use indices to avoid simultaneous mutable borrows.
            let mut expired: Vec<usize> = Vec::new();
            for (si, sing) in self.singularities.iter_mut().enumerate() {
                // Pull enemy ships.
                for ship in &mut self.ships {
                    if !ship.alive { continue; }
                    let dx = sing.pos.x - ship.pos.x;
                    let dy = sing.pos.y - ship.pos.y;
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq < sing_radius * sing_radius && dist_sq > 0.0 {
                        let dist = dist_sq.sqrt();
                        let nx = dx / dist;
                        let ny = dy / dist;
                        ship.vel.x += nx * sing_pull;
                        ship.vel.y += ny * sing_pull;
                        // Reclamp to effective_max_speed to avoid singularity exploits.
                        let spd = (ship.vel.x * ship.vel.x + ship.vel.y * ship.vel.y).sqrt();
                        let cap = if ship.afterburner_ticks_left > 0 {
                            self.params.max_speed * self.params.afterburner_speed_mult
                        } else {
                            self.params.max_speed
                        };
                        if spd > cap {
                            let s = cap / spd;
                            ship.vel.x *= s;
                            ship.vel.y *= s;
                        }
                    }
                }

                // Pull loose relics.
                for relic in &mut self.relics {
                    let dx = sing.pos.x - relic.pos.x;
                    let dy = sing.pos.y - relic.pos.y;
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq < sing_radius * sing_radius && dist_sq > 0.0 {
                        let dist = dist_sq.sqrt();
                        relic.pos.x += (dx / dist) * sing_pull;
                        relic.pos.y += (dy / dist) * sing_pull;
                    }
                }

                // Tick-down.
                if sing.ticks_left > 0 {
                    sing.ticks_left -= 1;
                }
                if sing.ticks_left == 0 {
                    expired.push(si);
                }
            }
            // Remove expired (in reverse order to preserve indices).
            for si in expired.into_iter().rev() {
                self.singularities.swap_remove(si);
            }
        }

        // ── Issue 10: Aether Mine arm-down, proximity check, detonation ──────
        //
        // Each tick:
        //   1. Decrement arm_ticks_left until it reaches 0 (mine is armed).
        //   2. For armed mines: check each alive enemy ship within mine_radius;
        //      if any, detonate (apply_env_damage with mine_damage; schedule
        //      respawn if lethal; emit MineDetonated to all ships in range).
        //   3. Remove detonated mines.
        {
            let mine_r    = self.params.mine_radius;
            let mine_dmg  = self.params.mine_damage;
            let respawn_d = self.params.respawn_delay;

            let mut mines_to_keep: Vec<MineState> = Vec::new();
            let mines = std::mem::take(&mut self.mines);

            for mut mine in mines {
                // Arm tick-down.
                if mine.arm_ticks_left > 0 {
                    mine.arm_ticks_left -= 1;
                    mines_to_keep.push(mine);
                    continue;
                }

                // Check for enemy ships within mine_radius.
                let mut detonated = false;
                let mut env_deaths: Vec<(ShipId, Vec2, u32)> = Vec::new();
                for ship in &mut self.ships {
                    if !ship.alive || ship.id == mine.owner {
                        continue;
                    }
                    let dx = ship.pos.x - mine.pos.x;
                    let dy = ship.pos.y - mine.pos.y;
                    if dx * dx + dy * dy < mine_r * mine_r {
                        detonated = true;
                        if apply_env_damage(ship, mine_dmg, &mut ship_events) {
                            env_deaths.push((ship.id.clone(), ship.pos, ship.relics_carried));
                            ship.relics_carried     = 0;
                            ship.respawn_ticks_left = respawn_d;
                        }
                        // Emit MineDetonated to the ship that triggered it.
                        ship_events
                            .entry(ship.id.clone())
                            .or_default()
                            .push(Event::MineDetonated {
                                mine_id: mine.id.clone(),
                                pos: mine.pos,
                            });
                    }
                }

                if detonated {
                    // Drop relics from mine-killed ships.
                    let scatter = self.params.ship_radius * 1.5;
                    for (id, pos, count) in env_deaths {
                        for _ in 0..count {
                            let angle: f32 = self.rng.random_range(0.0..TAU);
                            let r: f32     = self.rng.random_range(0.0..scatter);
                            let rp = Vec2::new(pos.x + angle.cos() * r, pos.y + angle.sin() * r);
                            let rid = format!("relic-{}", self.relic_id_counter);
                            self.relic_id_counter += 1;
                            self.relics.push(RelicState { id: rid.clone(), pos: rp });
                            ship_events
                                .entry(id.clone())
                                .or_default()
                                .push(Event::RelicDropped { relic_id: rid, pos: rp });
                        }
                    }
                    // Mine is consumed; do NOT push to mines_to_keep.
                } else {
                    mines_to_keep.push(mine);
                }
            }
            self.mines = mines_to_keep;
        }

        // ── Issue 10: Arc Lance bolt movement, hit detection, and damage ──────
        //
        // Lance bolts travel at lance_speed per tick, pierce through all ships
        // in their path, and bypass shields (damage goes directly to Hull).
        // They use the same proj_range limit as rune-cannon projectiles.
        {
            let ship_r_sq   = self.params.ship_radius * self.params.ship_radius;
            let lance_range = self.params.proj_range;
            let lance_dmg   = self.params.lance_damage;
            let respawn_d   = self.params.respawn_delay;

            let bolts = std::mem::take(&mut self.lance_bolts);
            let mut alive_bolts: Vec<LanceBoltState> = Vec::new();

            for mut bolt in bolts {
                // Move bolt.
                bolt.pos.x += bolt.vel.x;
                bolt.pos.y += bolt.vel.y;
                let step_d = (bolt.vel.x * bolt.vel.x + bolt.vel.y * bolt.vel.y).sqrt();
                bolt.dist_traveled += step_d;

                // Range despawn.
                if bolt.dist_traveled >= lance_range {
                    continue;
                }

                // Hit detection — pierce ALL ships in range (not just first).
                let mut env_deaths: Vec<(ShipId, Vec2, u32)> = Vec::new();
                for ship in &mut self.ships {
                    if !ship.alive || ship.id == bolt.owner {
                        continue;
                    }
                    let dx = ship.pos.x - bolt.pos.x;
                    let dy = ship.pos.y - bolt.pos.y;
                    if dx * dx + dy * dy < ship_r_sq {
                        if apply_lance_damage(
                            ship,
                            lance_dmg,
                            &bolt.owner,
                            &mut ship_events,
                        ) {
                            env_deaths.push((ship.id.clone(), ship.pos, ship.relics_carried));
                            ship.relics_carried     = 0;
                            ship.respawn_ticks_left = respawn_d;
                        }
                    }
                }

                // Drop relics for lance-killed ships.
                let scatter = self.params.ship_radius * 1.5;
                for (id, pos, count) in env_deaths {
                    for _ in 0..count {
                        let angle: f32 = self.rng.random_range(0.0..TAU);
                        let r: f32     = self.rng.random_range(0.0..scatter);
                        let rp = Vec2::new(pos.x + angle.cos() * r, pos.y + angle.sin() * r);
                        let rid = format!("relic-{}", self.relic_id_counter);
                        self.relic_id_counter += 1;
                        self.relics.push(RelicState { id: rid.clone(), pos: rp });
                        ship_events
                            .entry(id.clone())
                            .or_default()
                            .push(Event::RelicDropped { relic_id: rid, pos: rp });
                    }
                }

                // Bolt keeps flying (pierces — never removed on hit).
                alive_bolts.push(bolt);
            }
            self.lance_bolts = alive_bolts;
        }

        // ── Issue 06: invuln tick-down ────────────────────────────────────────
        //
        // Runs AFTER combat so that a ship's invuln flag is still set during the
        // combat block on every tick it is counted as "protected".
        for ship in &mut self.ships {
            if !ship.alive || !ship.invuln || ship.invuln_ticks_left == 0 {
                continue;
            }
            ship.invuln_ticks_left -= 1;
            if ship.invuln_ticks_left == 0 {
                ship.invuln = false;
            }
        }

        // ── Issue 09: Bulwark immunity tick-down ──────────────────────────────
        //
        // Runs in sync with the invuln tick-down above.  When `bulwark_ticks_left`
        // reaches 0 the `BulwarkExpired` event is emitted to the ship.
        for ship in &mut self.ships {
            if ship.bulwark_ticks_left == 0 {
                continue;
            }
            ship.bulwark_ticks_left -= 1;
            if ship.bulwark_ticks_left == 0 {
                ship_events
                    .entry(ship.id.clone())
                    .or_default()
                    .push(Event::BulwarkExpired);
            }
        }

        // ── Issue 09: Afterburner boost tick-down ─────────────────────────────
        //
        // Counts down separately from invuln.  Physics reads `afterburner_ticks_left`
        // at the START of each step (before the tick-down runs), so the "+1" set
        // at discharge ensures exactly `afterburner_dur` boosted physics steps.
        //   discharge step : tl = dur+1.  Physics: normal (discharge in 5_sigil,
        //                    after physics).  End-of-step: dur+1 → dur.
        //   boost step 1   : tl = dur.  Physics: boosted.  End: dur → dur-1.
        //   …
        //   boost step dur : tl = 1. Physics: boosted. End: 1 → 0. AfterburnerExpired.
        //   step dur+1     : tl = 0. Physics: normal.
        for ship in &mut self.ships {
            if ship.afterburner_ticks_left == 0 {
                continue;
            }
            ship.afterburner_ticks_left -= 1;
            if ship.afterburner_ticks_left == 0 {
                ship_events
                    .entry(ship.id.clone())
                    .or_default()
                    .push(Event::AfterburnerExpired);
            }
        }

        // 6. Record the applied (persisted) intent for every ship this tick.
        // `iter_mut` is needed so we can take (and clear) `sigil_discharge_this_tick`.
        let frame: IntentFrame = self
            .ships
            .iter_mut()
            .map(|s| {
                let discharge = s.sigil_discharge_this_tick.take();
                (s.id.clone(), s.applied_intent(discharge))
            })
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
            asteroids: self.asteroids.iter().map(|a| a.to_view()).collect(),
            projectiles: self.projectiles.iter().map(|p| p.to_view()).collect(),
            singularities: self
                .singularities
                .iter()
                .map(|s| s.to_view(self.params.singularity_radius))
                .collect(),
            // Mine visibility: owner always sees own mines (own=true);
            // enemy ships see a mine only when within mine_radius (proximity-visible).
            mines: {
                let observing_id = &ship.id;
                let mine_r_sq = self.params.mine_radius * self.params.mine_radius;
                self.mines
                    .iter()
                    .filter_map(|m| {
                        if &m.owner == observing_id {
                            Some(m.to_view(true))
                        } else {
                            let dx = ship.pos.x - m.pos.x;
                            let dy = ship.pos.y - m.pos.y;
                            if dx * dx + dy * dy <= mine_r_sq {
                                Some(m.to_view(false))
                            } else {
                                None // hidden from this observer
                            }
                        }
                    })
                    .collect()
            },
            scores: self.scores.clone(),
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
            asteroids: self.asteroids.iter().map(|a| a.to_view()).collect(),
            projectiles: self.projectiles.iter().map(|p| p.to_view()).collect(),
            singularities: self
                .singularities
                .iter()
                .map(|s| s.to_view(self.params.singularity_radius))
                .collect(),
            // God-view shows ALL mines (full state — for replay/viewer).
            mines: self.mines.iter().map(|m| m.to_view(true)).collect(),
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

    /// Test-only helper: set the `invuln` flag on a ship by id.
    ///
    /// This allows collision tests to verify that invulnerable ships receive no
    /// collision damage, without waiting for issue 06 (spawn protection) to land.
    /// Setting `invuln = true` with `invuln_ticks_left = 0` (default) means the
    /// flag persists indefinitely — it won't be expired by the invuln countdown.
    pub fn set_invuln_for_test(&mut self, ship_id: &str, invuln: bool) {
        if let Some(ship) = self.ships.iter_mut().find(|s| s.id == ship_id) {
            ship.invuln = invuln;
        }
    }

    /// Test-only helper: directly set `relics_carried` on a ship by id.
    ///
    /// Allows relic-drop tests to put the ship into a carrying state without
    /// running through a full pickup + non-banking scenario.
    pub fn set_relics_carried_for_test(&mut self, ship_id: &str, count: u32) {
        if let Some(ship) = self.ships.iter_mut().find(|s| s.id == ship_id) {
            ship.relics_carried = count;
        }
    }

    /// Test-only helper: directly set the held Sigil on a ship by id.
    ///
    /// Allows Sigil-effect tests to bypass the relic-pickup flow and grant
    /// a specific Sigil directly.  Mirrors the precedent of
    /// `set_invuln_for_test` / `set_relics_carried_for_test`.
    pub fn set_sigil_for_test(&mut self, ship_id: &str, sigil: Option<Sigil>) {
        if let Some(ship) = self.ships.iter_mut().find(|s| s.id == ship_id) {
            ship.sigil = sigil;
        }
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
    // Invuln ships (spawn-protection or Bulwark) take no damage from any source.
    if !ship.alive || damage <= 0.0 || ship.invuln {
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
            // Relic drop + respawn scheduling handled in the 5e kill-resolution
            // block (Engine::step), which has access to self.relics and self.rng.
            return Some(by.clone());
        }
    }

    None
}

/// Apply environmental (collision) `damage` to `ship`.
///
/// Shares the same Shield-then-Hull path as `apply_cannon_damage`, but:
/// - Respects `ship.invuln` (spawn-protection / Bulwark immunity): no damage
///   is applied and `false` is returned.
/// - Emits `CollisionTookShield` / `CollisionTookHull` instead of the
///   cannon variants (no `by` field — collision damage is environmental).
/// - Lethal hits emit `Died { by: None }` and do NOT award a kill bounty.
///
/// Returns `true` when the hit was lethal (Hull reached 0).  Callers are
/// responsible for scheduling respawn and dropping carried relics when `true`
/// is returned (see the env_death_drops pattern in `Engine::step`).
///
/// Seam for issue 10 (Aether Mine detonation): call `apply_env_damage` with
/// `params.mine_damage` once proximity triggers, then check the return value
/// to schedule respawn — the damage path is identical and decoupled.
fn apply_env_damage(
    ship: &mut ShipState,
    damage: f32,
    events: &mut HashMap<ShipId, Vec<Event>>,
) -> bool {
    if !ship.alive || damage <= 0.0 || ship.invuln {
        return false;
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
            .push(Event::CollisionTookShield { amount: shield_absorbed });
        if ship.shield.cur == 0.0 {
            events
                .entry(ship.id.clone())
                .or_default()
                .push(Event::ShieldDown);
        }
    }

    if hull_overflow > 0.0 {
        ship.hull.cur -= hull_overflow;
        if ship.hull.cur < 0.0 {
            ship.hull.cur = 0.0;
        }
        events
            .entry(ship.id.clone())
            .or_default()
            .push(Event::CollisionTookHull { amount: hull_overflow });
        if ship.hull.cur <= 0.0 {
            ship.alive = false;
            // Environmental death: no kill attribution, no score bounty.
            events
                .entry(ship.id.clone())
                .or_default()
                .push(Event::Died { by: None });
            return true;
        }
    }

    false
}

/// Apply Arc Lance `damage` to `ship`, bypassing the Shield entirely.
///
/// The Arc Lance is a special piercing weapon that deals damage directly to
/// the Hull, ignoring the Shield value.  Respects spawn-protection / Bulwark
/// invulnerability (same guard as all other damage sources).
///
/// Emits `LanceTookHull { amount, by }` to the target.
/// Emits `Died { by: None }` on lethal hit (lance kills are unattributed —
/// no kill bounty; the weapon is too powerful to award score for).
///
/// Returns `true` when the hit was lethal.  Caller handles respawn scheduling
/// and relic drops.
fn apply_lance_damage(
    ship: &mut ShipState,
    damage: f32,
    by: &ShipId,
    events: &mut HashMap<ShipId, Vec<Event>>,
) -> bool {
    if !ship.alive || damage <= 0.0 || ship.invuln {
        return false;
    }

    // Reset shield-regen delay.
    ship.ticks_since_last_hit = 0;

    // Direct hull damage — Shield is completely bypassed.
    let applied = damage.min(ship.hull.cur.max(0.0));
    ship.hull.cur -= applied;
    if ship.hull.cur < 0.0 {
        ship.hull.cur = 0.0;
    }

    events
        .entry(ship.id.clone())
        .or_default()
        .push(Event::LanceTookHull { amount: applied, by: by.clone() });

    if ship.hull.cur <= 0.0 {
        ship.alive = false;
        events
            .entry(ship.id.clone())
            .or_default()
            .push(Event::Died { by: None });
        return true;
    }

    false
}
