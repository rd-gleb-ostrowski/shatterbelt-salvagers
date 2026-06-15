"""First-pass balance parameters for Shatterbelt Salvagers.

Every number here is a tunable starting point. The harness (harness.py) reports
metrics and runs simulated matches so we can adjust these from evidence.
Units: arena units for distance, ticks for time (30 ticks/second).
"""
from dataclasses import dataclass


@dataclass
class Params:
    # --- match setup ---
    tick_rate: int = 30
    arena_w: float = 2000.0
    arena_h: float = 1200.0
    max_ticks: int = 3600          # ~2 minutes
    n_asteroids: int = 10
    asteroid_radius_min: float = 40.0
    asteroid_radius_max: float = 90.0
    asteroid_drift: float = 1.0

    # --- ship & movement ---
    ship_radius: float = 20.0
    max_speed: float = 12.0
    thrust_accel: float = 0.5      # units/tick^2 at full thrust
    reverse_accel: float = 0.25
    lin_damping: float = 0.97      # velocity *= this each tick
    max_turn: float = 0.15         # rad/tick

    # --- aether ---
    aether_max: float = 100.0
    aether_regen: float = 1.2      # per tick
    thrust_cost_full: float = 1.0  # per tick at full thrust
    shot_cost: float = 12.0

    # --- combat ---
    cannon_damage: float = 20.0
    proj_speed: float = 25.0       # units/tick
    proj_range: float = 1500.0
    cannon_cooldown: int = 15      # ticks between shots
    cannon_start_hot: int = 15     # initial cooldown each life
    shield_max: float = 60.0
    shield_regen: float = 2.0      # per tick, after delay
    shield_regen_delay: int = 30   # ticks unhit before regen resumes
    hull_max: float = 100.0

    # --- collisions: damage = (impact_speed - threshold) * k, min 0 ---
    coll_threshold: float = 4.0
    k_asteroid: float = 5.0
    k_ram: float = 3.0
    k_wall: float = 3.0

    # --- relics, scoring, respawn ---
    relic_value: float = 1.0
    kill_bounty: float = 2.0        # direct score for a kill (no extra relic drop)
    carry_cap: int = 5
    relic_spawn_period: int = 60
    relic_field_cap: int = 12
    respawn_delay: int = 90

    # --- sigils (analytic only for now) ---
    afterburner_dur: int = 30
    afterburner_thrust_mult: float = 3.0
    afterburner_speed_mult: float = 1.5
    bulwark_immunity: int = 45
    singularity_radius: float = 200.0
    singularity_pull: float = 0.6
    singularity_dur: int = 60
    mine_arm: int = 15
    mine_radius: float = 40.0
    mine_damage: float = 60.0
    lance_speed: float = 40.0
    lance_damage: float = 50.0

    # --- harness toggle ---
    enable_sigils: bool = True


DEFAULT = Params()
