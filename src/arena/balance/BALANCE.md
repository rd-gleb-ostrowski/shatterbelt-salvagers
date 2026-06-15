# Arena Balance — Shatterbelt Salvagers (first pass)

These are the **first-pass gameplay numbers**, tuned with the headless harness in this folder
(`harness.py`). They are a defensible starting point, **not** final — they still need real
playtests with real bots. The authoritative values live in [`params.py`](./params.py); this
document explains the design rules and what the harness tells us.

## Locked design rules

- **Scoring:** economic + respawn. Score = **relic value banked** at your Anchor, **plus a kill
  bounty** per kill (no extra relic drop — a destroyed ship only drops what it was carrying).
  Most score at `maxTicks` wins. (ADR-style decision; supersedes the deck's old "last fleet
  standing" line.)
- **Respawn:** a destroyed ship drops its carried relics into the Drift and respawns at its
  Anchor after `respawn_delay` ticks with full stats — nobody is eliminated early.
- **Movement:** light-damping drift (`lin_damping`) + a `max_speed` cap. You coast, but settle.
  Turn is rate-controlled; thrust is force/momentum and costs aether.
- **Collisions:** walled arena; asteroids, ship-rams, and walls bounce and deal damage scaled to
  impact speed above a small threshold.

## Tuned numbers (see `params.py` for the live values)

| group | key values |
|---|---|
| Match | arena 2000×1200, 2–6 ships (FFA), `maxTicks` 3600 (~2 min), ~10 asteroids |
| Movement | max speed 12, thrust accel 0.5, **damping 0.97**, max turn 0.15 rad/tick |
| Aether | max 100, **regen 1.2/tick**, full-thrust cost 1.0/tick, shot cost 12 |
| Combat | cannon 20 dmg, proj 25/tick (~1500 range), cooldown 15 (starts hot), shield 60 (+2/tick after 30t unhit), hull 100 |
| Collisions | damage = (impactSpeed − 4) × k: asteroid 5, ram 3, wall 3 |
| Relics | value 1, carry cap 5, +1 every 60t (cap 12 on field), **kill bounty 2** |
| Respawn | drop carried relics, respawn at Anchor after 90t |
| Sigils | Afterburner 30t ×3 thrust/+50% speed; Bulwark full shield +45t immunity; Singularity r200 pull 0.6 for 60t; Mine arms 15t, r40, 60 dmg; Arc Lance 40/tick, 50 dmg, shield-bypassing |

## What the harness reports at these values

- **Kinematics:** coast-to-stop ~**384 units** (3.5s), cross the arena width in ~5.6s, full
  360° turn in ~1.4s. (At the old damping 0.99 the stop distance was 1150u — far too floaty.)
- **Aether:** net **+0.2/tick** at full thrust; while coasting the cannon is cooldown-limited
  (can afford a shot every ~10t < 15t cooldown), while maneuvering it's mildly aether-limited —
  the intended move-vs-fight trade without starving you.
- **Combat:** **TTK ≈ 4.0s** (in the 2–8s sweet spot); shields do **not** out-regen sustained
  fire, so ships are killable; ~96 aether per kill.
- **Sigils (as a fraction of a 160-EHP target):** Mine ~38% EHP, Arc Lance 50% of hull
  (shield-bypassing), Bulwark restores ~138% of base EHP + immunity, Afterburner 12 → 18 speed.
- **Match sims (heuristic bots, no sigils):** leaders bank ~20–30 over 2 min, matches are mostly
  decisive with **no shutouts**; with a kill bounty, an aggressor policy scores via denial and
  FFA sees ~2.7 kills/match. Two pure-salvager bots barely fight (expected — neither tries to).

## Caveats & open questions (need real playtests)

- The match sim is **approximate**: simplified point physics, no Sigils, and deliberately simple
  bots. Use it for *magnitudes* (drift, TTK, aether tension, sigil ratios), not for fine
  emergent balance.
- How much combat actually happens depends heavily on **bot intelligence** — smart agent bots
  will fight far more than the heuristics here.
- Still unvalidated by simulation: Sigil interactions, asteroid-cover tactics, the carry-cap vs
  banking-trip risk/reward, and multi-ship (5–6) FFA crowding.

## Running the harness

```
cd src/arena/balance
python3 harness.py          # prints the full report for params.DEFAULT
```

Tune by editing `params.py` (or `dataclasses.replace(DEFAULT, ...)` in a scratch script) and
re-running. The report flags suspicious values (excessive drift, unkillable shields, TTK outside
2–8s, matches with no kills or all shutouts).
