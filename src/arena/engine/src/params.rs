/// All gameplay numbers for a Match, mirroring `src/arena/balance/params.py`.
///
/// Every number here is a tunable first-pass value.  Engine logic must never
/// hardcode gameplay constants — read them from `Params` instead.
/// Units: arena units for distance, ticks for time (30 ticks/second default).
#[derive(Debug, Clone)]
pub struct Params {
    // ── Match setup ──────────────────────────────────────────────────────────
    pub tick_rate: u32,
    pub arena_w: f32,
    pub arena_h: f32,
    pub max_ticks: u32,
    pub n_asteroids: u32,
    pub asteroid_radius_min: f32,
    pub asteroid_radius_max: f32,
    pub asteroid_drift: f32,

    // ── Ship & movement ──────────────────────────────────────────────────────
    pub ship_radius: f32,
    pub max_speed: f32,
    pub thrust_accel: f32,   // units/tick² at full thrust
    pub reverse_accel: f32,
    pub lin_damping: f32,    // velocity *= lin_damping each tick
    pub max_turn: f32,       // rad/tick

    // ── Aether ───────────────────────────────────────────────────────────────
    pub aether_max: f32,
    pub aether_regen: f32,       // per tick
    pub thrust_cost_full: f32,   // per tick at full thrust
    pub shot_cost: f32,

    // ── Combat ───────────────────────────────────────────────────────────────
    pub cannon_damage: f32,
    pub proj_speed: f32,         // units/tick
    pub proj_range: f32,
    pub cannon_cooldown: u32,    // ticks between shots
    pub cannon_start_hot: u32,   // initial cannon cooldown at spawn
    pub shield_max: f32,
    pub shield_regen: f32,       // per tick, after delay
    pub shield_regen_delay: u32, // ticks unhit before regen resumes
    pub hull_max: f32,

    // ── Collisions: damage = max(0, (impact_speed − threshold) × k) ─────────
    pub coll_threshold: f32,
    pub k_asteroid: f32,
    pub k_ram: f32,
    pub k_wall: f32,

    // ── Relics, scoring, respawn ─────────────────────────────────────────────
    pub relic_value: f32,
    pub kill_bounty: f32,        // direct score awarded for a kill
    pub carry_cap: u32,
    pub relic_spawn_period: u32,
    pub relic_field_cap: u32,
    /// Pickup radius: a ship must be within this distance of a Relic to pick it up.
    /// Mirrors harness.py: ship_radius + 12.
    pub relic_pickup_radius: f32,
    /// Banking radius: a ship must be within this distance of its Anchor to bank Relics.
    /// Mirrors harness.py: 60 units.
    pub anchor_bank_radius: f32,
    pub respawn_delay: u32,
    pub respawn_invuln: u32,     // ticks of spawn-protection after respawn

    // ── Sigils ───────────────────────────────────────────────────────────────
    pub afterburner_dur: u32,
    pub afterburner_thrust_mult: f32,
    pub afterburner_speed_mult: f32,
    pub bulwark_immunity: u32,
    pub singularity_radius: f32,
    pub singularity_pull: f32,
    pub singularity_dur: u32,
    pub mine_arm: u32,
    pub mine_radius: f32,
    pub mine_damage: f32,
    pub lance_speed: f32,
    pub lance_damage: f32,

    // ── Harness ──────────────────────────────────────────────────────────────
    pub enable_sigils: bool,
}

impl Default for Params {
    /// First-pass values, mirrored exactly from `src/arena/balance/params.py`.
    fn default() -> Self {
        Self {
            tick_rate: 30,
            arena_w: 2000.0,
            arena_h: 1200.0,
            max_ticks: 3600,
            n_asteroids: 10,
            asteroid_radius_min: 40.0,
            asteroid_radius_max: 90.0,
            asteroid_drift: 1.0,

            ship_radius: 20.0,
            max_speed: 12.0,
            thrust_accel: 0.5,
            reverse_accel: 0.25,
            lin_damping: 0.97,
            max_turn: 0.15,

            aether_max: 100.0,
            aether_regen: 1.2,
            thrust_cost_full: 1.0,
            shot_cost: 12.0,

            cannon_damage: 20.0,
            proj_speed: 25.0,
            proj_range: 1500.0,
            cannon_cooldown: 15,
            cannon_start_hot: 15,
            shield_max: 60.0,
            shield_regen: 2.0,
            shield_regen_delay: 30,
            hull_max: 100.0,

            coll_threshold: 4.0,
            k_asteroid: 5.0,
            k_ram: 3.0,
            k_wall: 3.0,

            relic_value: 1.0,
            kill_bounty: 2.0,
            carry_cap: 5,
            relic_spawn_period: 60,
            relic_field_cap: 12,
            relic_pickup_radius: 32.0,   // ship_radius(20) + 12, mirrors harness.py
            anchor_bank_radius: 60.0,    // mirrors harness.py
            respawn_delay: 90,
            respawn_invuln: 45,

            afterburner_dur: 30,
            afterburner_thrust_mult: 3.0,
            afterburner_speed_mult: 1.5,
            bulwark_immunity: 45,
            singularity_radius: 200.0,
            singularity_pull: 0.6,
            singularity_dur: 60,
            mine_arm: 15,
            mine_radius: 40.0,
            mine_damage: 60.0,
            lance_speed: 40.0,
            lance_damage: 50.0,

            enable_sigils: true,
        }
    }
}
