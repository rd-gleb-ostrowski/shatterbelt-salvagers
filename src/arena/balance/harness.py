"""Headless balance harness for Shatterbelt Salvagers.

Two parts:
  1) analytic metrics   -- closed-form / single-ship sims of the key quantities
  2) match simulations  -- simplified 2D matches with heuristic bots, batched

Run:  python3 harness.py            (uses DEFAULT params)
Everything is deterministic given a seed. The match sim is intentionally
approximate (no sigils yet) -- it's a balance probe, not the real engine.
"""
import math
import random
import statistics
from dataclasses import replace

from params import DEFAULT, Params

TAU = 2 * math.pi


# --------------------------------------------------------------------------
# analytic metrics
# --------------------------------------------------------------------------
def kinematics(p: Params):
    # terminal speed under full thrust with damping, capped:
    # v_{t+1} = (v_t + a) * damping  -> steady state v* = a*d/(1-d)
    v_term_uncapped = p.thrust_accel * p.lin_damping / (1 - p.lin_damping)
    v_term = min(v_term_uncapped, p.max_speed)

    # time to reach 99% of terminal (cap-aware)
    v, t_acc = 0.0, 0
    while v < 0.99 * v_term and t_acc < 100000:
        v = (v + p.thrust_accel) * p.lin_damping
        v = min(v, p.max_speed)
        t_acc += 1

    # coast from max speed: time and distance to "stopped" (< 0.5 u/tick)
    v, dist, t_stop = p.max_speed, 0.0, 0
    while v > 0.5 and t_stop < 100000:
        dist += v
        v *= p.lin_damping
        t_stop += 1

    cross_w = p.arena_w / v_term            # ticks to cross at terminal speed
    full_turn = TAU / p.max_turn
    return dict(
        v_term_uncapped=v_term_uncapped, v_term=v_term, cap_binds=v_term_uncapped > p.max_speed,
        t_to_top=t_acc, stop_dist=dist, t_stop=t_stop,
        cross_arena_w=cross_w, full_turn=full_turn,
    )


def aether_metrics(p: Params):
    # net aether per tick at full thrust (no firing)
    net_full = p.aether_regen - p.thrust_cost_full
    # at thrust fraction f, surplus = regen - f*cost; ticks to afford a shot
    def fire_interval(f):
        surplus = p.aether_regen - f * p.thrust_cost_full
        if surplus <= 0:
            return math.inf
        return p.shot_cost / surplus  # ticks of saving per shot (cooldown-limited too)
    return dict(
        net_at_full_thrust=net_full,
        fire_interval_coast=fire_interval(0.0),
        fire_interval_half=fire_interval(0.5),
        fire_interval_full=fire_interval(1.0),
        cooldown=p.cannon_cooldown,
    )


def combat_metrics(p: Params):
    ehp = p.shield_max + p.hull_max
    shots_to_kill = math.ceil(ehp / p.cannon_damage)
    ttk_ticks = shots_to_kill * p.cannon_cooldown
    cannon_dps = p.cannon_damage / p.cannon_cooldown
    # can sustained fire out-pace shield regen? (regen only after delay unhit;
    # if cooldown < delay, shields never regen during a fight)
    regen_during_fight = p.shield_regen if p.cannon_cooldown >= p.shield_regen_delay else 0.0
    net_dps_vs_shield = cannon_dps - regen_during_fight
    aether_per_kill = shots_to_kill * p.shot_cost
    return dict(
        ehp=ehp, shots_to_kill=shots_to_kill, ttk_ticks=ttk_ticks,
        ttk_seconds=ttk_ticks / p.tick_rate, cannon_dps=cannon_dps,
        shield_regenerates_in_fight=regen_during_fight > 0,
        net_dps_vs_shield=net_dps_vs_shield, aether_per_kill=aether_per_kill,
    )


def sigil_metrics(p: Params):
    ehp = p.shield_max + p.hull_max
    return dict(
        mine_pct_ehp=p.mine_damage / ehp,
        lance_pct_hull=p.lance_damage / p.hull_max,   # bypasses shield
        bulwark_ehp_swing=(p.shield_max + p.hull_max + p.shield_max) / ehp,
        afterburner_speed=p.max_speed * p.afterburner_speed_mult,
    )


# --------------------------------------------------------------------------
# match simulation
# --------------------------------------------------------------------------
def clampf(x, lo, hi):
    return lo if x < lo else hi if x > hi else x


def norm_angle(a):
    while a > math.pi:
        a -= TAU
    while a < -math.pi:
        a += TAU
    return a


class Ship:
    __slots__ = ("id", "policy", "p", "x", "y", "vx", "vy", "heading", "hull", "shield",
                 "aether", "cd", "unhit", "alive", "respawn", "carry", "score",
                 "kills", "deaths", "ax", "ay")

    def __init__(self, id, policy, ax, ay, p):
        self.id, self.policy, self.p = id, policy, p
        self.ax, self.ay = ax, ay      # anchor
        self.score = self.kills = self.deaths = 0
        self.spawn()

    def spawn(self):
        p = self.p
        self.x, self.y = self.ax, self.ay
        self.vx = self.vy = 0.0
        self.heading = 0.0
        self.alive = True
        self.respawn = 0
        self.carry = 0
        self.hull = p.hull_max
        self.shield = p.shield_max
        self.aether = p.aether_max
        self.cd = p.cannon_start_hot
        self.unhit = p.shield_regen_delay


class Proj:
    __slots__ = ("x", "y", "vx", "vy", "owner", "dist")

    def __init__(self, x, y, vx, vy, owner):
        self.x, self.y, self.vx, self.vy, self.owner, self.dist = x, y, vx, vy, owner, 0.0


class World:
    def __init__(self, p: Params, policies, seed=0):
        self.p = p
        self.rng = random.Random(seed)
        self.ships = []
        n = len(policies)
        for i, pol in enumerate(policies):
            ang = TAU * i / n
            ax = p.arena_w / 2 + math.cos(ang) * p.arena_w * 0.4
            ay = p.arena_h / 2 + math.sin(ang) * p.arena_h * 0.4
            self.ships.append(Ship(i, pol, clampf(ax, 50, p.arena_w - 50),
                                   clampf(ay, 50, p.arena_h - 50), p))
        self.projs = []
        self.relics = []
        self.asteroids = []
        for _ in range(p.n_asteroids):
            self.asteroids.append([
                self.rng.uniform(0, p.arena_w), self.rng.uniform(0, p.arena_h),
                self.rng.uniform(p.asteroid_radius_min, p.asteroid_radius_max),
                self.rng.uniform(-1, 1) * p.asteroid_drift,
                self.rng.uniform(-1, 1) * p.asteroid_drift,
            ])
        for _ in range(6):
            self.spawn_relic()
        self.first_kill = None

    def spawn_relic(self):
        if len(self.relics) >= self.p.relic_field_cap:
            return
        self.relics.append([self.rng.uniform(100, self.p.arena_w - 100),
                            self.rng.uniform(100, self.p.arena_h - 100)])

    # --- bot policies: return (turn[-1..1], thrust[-1..1], fire bool) ---
    def decide(self, s: Ship):
        p = self.p
        enemies = [o for o in self.ships if o is not s and o.alive]
        nearest_enemy = min(enemies, key=lambda o: (o.x - s.x) ** 2 + (o.y - s.y) ** 2, default=None)

        if s.policy == "aggressor" and nearest_enemy:
            tx, ty = nearest_enemy.x, nearest_enemy.y
        else:  # salvager
            if s.carry >= p.carry_cap or (s.carry > 0 and not self.relics):
                tx, ty = s.ax, s.ay
            elif self.relics:
                r = min(self.relics, key=lambda r: (r[0] - s.x) ** 2 + (r[1] - s.y) ** 2)
                tx, ty = r[0], r[1]
            elif nearest_enemy:
                tx, ty = nearest_enemy.x, nearest_enemy.y
            else:
                tx, ty = s.ax, s.ay

        desired = math.atan2(ty - s.y, tx - s.x)
        err = norm_angle(desired - s.heading)
        turn = clampf(err / p.max_turn, -1, 1)
        thrust = 1.0 if abs(err) < 0.5 else 0.3

        fire = False
        if nearest_enemy is not None:
            ea = math.atan2(nearest_enemy.y - s.y, nearest_enemy.x - s.x)
            d = math.hypot(nearest_enemy.x - s.x, nearest_enemy.y - s.y)
            if abs(norm_angle(ea - s.heading)) < 0.12 and d < p.proj_range and s.aether >= p.shot_cost:
                fire = True
        return turn, thrust, fire

    def damage(self, s: Ship, dmg, attacker=None, tick=0):
        if dmg <= 0 or not s.alive:
            return
        s.unhit = 0
        if s.shield >= dmg:
            s.shield -= dmg
            return
        dmg -= s.shield
        s.shield = 0.0
        s.hull -= dmg
        if s.hull <= 0:
            s.alive = False
            s.respawn = self.p.respawn_delay
            s.deaths += 1
            for _ in range(s.carry):
                self.relics.append([s.x + self.rng.uniform(-30, 30), s.y + self.rng.uniform(-30, 30)])
            s.carry = 0
            if attacker is not None:
                attacker.kills += 1
                attacker.score += self.p.kill_bounty
                if self.first_kill is None:
                    self.first_kill = tick

    def step(self, tick):
        p = self.p
        # asteroids drift
        for a in self.asteroids:
            a[0] = (a[0] + a[3]) % p.arena_w
            a[1] = (a[1] + a[4]) % p.arena_h
        # ships
        for s in self.ships:
            if not s.alive:
                s.respawn -= 1
                if s.respawn <= 0:
                    s.spawn()
                    s.hull = p.hull_max
                    s.shield = p.shield_max
                    s.aether = p.aether_max
                    s.cd = p.cannon_start_hot
                    s.unhit = p.shield_regen_delay
                continue
            turn, thrust, fire = self.decide(s)
            s.heading = (s.heading + turn * p.max_turn) % TAU
            acc = (thrust * p.thrust_accel) if thrust >= 0 else (thrust * p.reverse_accel)
            s.vx += math.cos(s.heading) * acc
            s.vy += math.sin(s.heading) * acc
            s.vx *= p.lin_damping
            s.vy *= p.lin_damping
            sp = math.hypot(s.vx, s.vy)
            if sp > p.max_speed:
                s.vx *= p.max_speed / sp
                s.vy *= p.max_speed / sp
            s.x += s.vx
            s.y += s.vy
            # aether
            s.aether = clampf(s.aether + p.aether_regen - abs(thrust) * p.thrust_cost_full, 0, p.aether_max)
            # shield regen
            s.unhit += 1
            if s.unhit >= p.shield_regen_delay:
                s.shield = clampf(s.shield + p.shield_regen, 0, p.shield_max)
            # cannon
            if s.cd > 0:
                s.cd -= 1
            if fire and s.cd == 0 and s.aether >= p.shot_cost:
                self.projs.append(Proj(s.x, s.y, math.cos(s.heading) * p.proj_speed,
                                       math.sin(s.heading) * p.proj_speed, s.id))
                s.aether -= p.shot_cost
                s.cd = p.cannon_cooldown
            # walls
            if s.x < p.ship_radius:
                s.x = p.ship_radius; self.damage(s, max(0, abs(s.vx) - p.coll_threshold) * p.k_wall); s.vx = -s.vx * 0.5
            if s.x > p.arena_w - p.ship_radius:
                s.x = p.arena_w - p.ship_radius; self.damage(s, max(0, abs(s.vx) - p.coll_threshold) * p.k_wall); s.vx = -s.vx * 0.5
            if s.y < p.ship_radius:
                s.y = p.ship_radius; self.damage(s, max(0, abs(s.vy) - p.coll_threshold) * p.k_wall); s.vy = -s.vy * 0.5
            if s.y > p.arena_h - p.ship_radius:
                s.y = p.arena_h - p.ship_radius; self.damage(s, max(0, abs(s.vy) - p.coll_threshold) * p.k_wall); s.vy = -s.vy * 0.5
            # asteroid collisions
            for a in self.asteroids:
                dx, dy = s.x - a[0], s.y - a[1]
                d = math.hypot(dx, dy)
                rr = p.ship_radius + a[2]
                if d < rr and d > 0:
                    nx, ny = dx / d, dy / d
                    s.x, s.y = a[0] + nx * rr, a[1] + ny * rr
                    vn = s.vx * nx + s.vy * ny
                    self.damage(s, max(0, abs(vn) - p.coll_threshold) * p.k_asteroid)
                    s.vx -= 2 * vn * nx; s.vy -= 2 * vn * ny
            # relic pickup / banking
            if s.carry < p.carry_cap:
                for r in list(self.relics):
                    if (r[0] - s.x) ** 2 + (r[1] - s.y) ** 2 < (p.ship_radius + 12) ** 2:
                        self.relics.remove(r); s.carry += 1
                        if s.carry >= p.carry_cap:
                            break
            if s.carry > 0 and (s.x - s.ax) ** 2 + (s.y - s.ay) ** 2 < 60 ** 2:
                s.score += s.carry * p.relic_value; s.carry = 0

        # ship-ship collisions
        for i in range(len(self.ships)):
            for j in range(i + 1, len(self.ships)):
                a, b = self.ships[i], self.ships[j]
                if not (a.alive and b.alive):
                    continue
                dx, dy = b.x - a.x, b.y - a.y
                d = math.hypot(dx, dy)
                if 0 < d < 2 * p.ship_radius:
                    nx, ny = dx / d, dy / d
                    closing = (a.vx - b.vx) * nx + (a.vy - b.vy) * ny
                    dmg = max(0, abs(closing) - p.coll_threshold) * p.k_ram
                    self.damage(a, dmg); self.damage(b, dmg)
                    a.vx -= closing * nx; a.vy -= closing * ny
                    b.vx += closing * nx; b.vy += closing * ny

        # projectiles
        alive_projs = []
        for pr in self.projs:
            pr.x += pr.vx; pr.y += pr.vy; pr.dist += p.proj_speed
            if pr.dist > p.proj_range or not (0 <= pr.x <= p.arena_w and 0 <= pr.y <= p.arena_h):
                continue
            hit = False
            for s in self.ships:
                if s.alive and s.id != pr.owner and (s.x - pr.x) ** 2 + (s.y - pr.y) ** 2 < p.ship_radius ** 2:
                    owner = next((o for o in self.ships if o.id == pr.owner), None)
                    self.damage(s, p.cannon_damage, owner, tick)
                    hit = True
                    break
            if not hit:
                for a in self.asteroids:
                    if (a[0] - pr.x) ** 2 + (a[1] - pr.y) ** 2 < a[2] ** 2:
                        hit = True; break
            if not hit:
                alive_projs.append(pr)
        self.projs = alive_projs

        if tick % p.relic_spawn_period == 0:
            self.spawn_relic()


def run_match(p: Params, policies, seed):
    w = World(p, policies, seed)
    for t in range(1, p.max_ticks + 1):
        w.step(t)
    ships = w.ships
    scores = [s.score for s in ships]
    leader = max(scores)
    second = sorted(scores)[-2] if len(scores) > 1 else 0
    return dict(
        scores=scores, leader=leader, margin=leader - second,
        kills=sum(s.kills for s in ships), deaths=sum(s.deaths for s in ships),
        first_kill=w.first_kill,
    )


def run_batch(p: Params, policies, n=60, seed0=0):
    rows = [run_match(p, policies, seed0 + i) for i in range(n)]
    leaders = [r["leader"] for r in rows]
    margins = [r["margin"] for r in rows]
    kills = [r["kills"] for r in rows]
    fk = [r["first_kill"] for r in rows if r["first_kill"] is not None]
    decisive = sum(1 for r in rows if r["leader"] > 0 and r["margin"] >= max(1, 0.2 * r["leader"]))
    shutout = sum(1 for r in rows if r["leader"] == 0)
    return dict(
        n=n, leader_mean=statistics.mean(leaders), leader_max=max(leaders),
        margin_mean=statistics.mean(margins), kills_mean=statistics.mean(kills),
        first_kill_mean=(statistics.mean(fk) if fk else None),
        no_kill_matches=n - len(fk),
        decisive_pct=100 * decisive / n, shutout_pct=100 * shutout / n,
    )


# --------------------------------------------------------------------------
# report
# --------------------------------------------------------------------------
def flag(cond, msg):
    return ("  \u26a0 " + msg) if cond else ""


def report(p: Params):
    print("=" * 70)
    print("SHATTERBELT SALVAGERS — balance report (first-pass numbers)")
    print("=" * 70)

    k = kinematics(p)
    print("\n[KINEMATICS]")
    print(f"  terminal speed (uncapped)   {k['v_term_uncapped']:.1f}  -> capped to {k['v_term']:.1f}"
          + flag(not k["cap_binds"], "speed cap never reached; damping/accel limit it"))
    print(f"  time to top speed           {k['t_to_top']} ticks ({k['t_to_top']/p.tick_rate:.1f}s)")
    print(f"  coast-to-stop distance      {k['stop_dist']:.0f} units, {k['t_stop']} ticks ({k['t_stop']/p.tick_rate:.1f}s)"
          + flag(k["stop_dist"] > p.arena_w / 3, f"huge drift (> arena/3 = {p.arena_w/3:.0f}); lower damping?"))
    print(f"  cross arena width           {k['cross_arena_w']:.0f} ticks ({k['cross_arena_w']/p.tick_rate:.1f}s)")
    print(f"  full 360° turn              {k['full_turn']:.0f} ticks ({k['full_turn']/p.tick_rate:.1f}s)")

    a = aether_metrics(p)
    print("\n[AETHER]")
    print(f"  net at full thrust          {a['net_at_full_thrust']:+.2f}/tick"
          + flag(a["net_at_full_thrust"] < 0, "full thrust drains aether (can't sustain burn)"))
    print(f"  ticks to afford a shot  coast={a['fire_interval_coast']:.0f}  half={a['fire_interval_half']:.0f}  full={a['fire_interval_full']}")
    print(f"  cannon cooldown             {a['cooldown']} ticks"
          + flag(a["fire_interval_half"] > a["cooldown"], "aether, not cooldown, limits fire rate while moving"))

    c = combat_metrics(p)
    print("\n[COMBAT]")
    print(f"  effective HP (shield+hull)  {c['ehp']:.0f}")
    print(f"  shots to kill               {c['shots_to_kill']}  -> TTK {c['ttk_ticks']} ticks ({c['ttk_seconds']:.1f}s)"
          + flag(not 2 <= c["ttk_seconds"] <= 8, "TTK outside the 2–8s sweet spot"))
    print(f"  cannon DPS                  {c['cannon_dps']:.2f}/tick")
    print(f"  shield regenerates in fight {c['shield_regenerates_in_fight']}"
          + flag(c["net_dps_vs_shield"] <= 0, "shield out-regens your fire — ships may be unkillable"))
    print(f"  aether per kill             {c['aether_per_kill']:.0f}")

    s = sigil_metrics(p)
    print("\n[SIGILS] (fraction of a target's effective HP / hull)")
    print(f"  Aether Mine   {s['mine_pct_ehp']*100:.0f}% of EHP"
          + flag(s["mine_pct_ehp"] > 0.6, "very high burst"))
    print(f"  Arc Lance     {s['lance_pct_hull']*100:.0f}% of hull (bypasses shield)")
    print(f"  Bulwark       restores to {s['bulwark_ehp_swing']*100:.0f}% of base EHP + immunity")
    print(f"  Afterburner   speed {p.max_speed:.0f} -> {s['afterburner_speed']:.0f}")

    print("\n[MATCH SIMS]  (heuristic bots, no sigils)")
    for label, pol in [
        ("1v1  salvager vs salvager", ["salvager", "salvager"]),
        ("1v1  salvager vs aggressor", ["salvager", "aggressor"]),
        ("4-FFA  3 salvager + 1 aggressor", ["salvager", "salvager", "salvager", "aggressor"]),
    ]:
        b = run_batch(p, pol, n=40)
        fk = f"{b['first_kill_mean']/p.tick_rate:.1f}s" if b["first_kill_mean"] else "—"
        print(f"  {label}")
        print(f"     leader score ~{b['leader_mean']:.1f} (max {b['leader_max']:.0f}), "
              f"margin ~{b['margin_mean']:.1f}, kills/match ~{b['kills_mean']:.1f}, "
              f"1st kill {fk}")
        print(f"     decisive {b['decisive_pct']:.0f}%, shutouts {b['shutout_pct']:.0f}%, "
              f"no-kill matches {b['no_kill_matches']}/{b['n']}"
              + flag(b["shutout_pct"] > 25, "many 0-score matches — relics too sparse / banking too hard")
              + flag(b["kills_mean"] < 1, "almost no kills — combat irrelevant"))
    print("\n" + "=" * 70)


if __name__ == "__main__":
    report(DEFAULT)
