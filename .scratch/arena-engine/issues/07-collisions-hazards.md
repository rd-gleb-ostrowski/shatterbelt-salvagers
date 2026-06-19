# Collisions & hazards

Status: ready-for-agent
Type: AFK
User stories: 18

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

Make the Drift a real hazard. End-to-end through physics contact → bounce → impact-scaled damage →
events/Observation:

- **Asteroids** populate the Drift (count scaling with N at constant density) as both collision
  hazard and cover.
- Collisions between a ship and an asteroid, another ship, or an **arena wall** bounce the ship and
  deal **damage scaled to impact speed** (faster impact → more damage), routed through the
  Shield-then-Hull model.
- Collision damage respects spawn-protection invulnerability and can contribute to destruction
  (an environmental death, no bounty).
- Collisions surface as per-ship events.

Asteroid count, restitution/bounce, and the impact-speed → damage curve come from the parameter
set.

## Acceptance criteria

- [ ] Asteroids spawn in the Drift with population scaling with ship count N.
- [ ] Ship–asteroid, ship–ship, and ship–wall collisions bounce the ship via the deterministic
      physics.
- [ ] Collision damage scales with impact speed and routes through Shield then Hull.
- [ ] An invulnerable (spawn-protected) ship takes no collision damage.
- [ ] Collisions emit per-ship events.
- [ ] Golden scenario: asteroid and wall collisions damage a ship in proportion to impact speed
      (mirroring the Python harness's magnitudes).

## Blocked by

- `02-drift-movement-piloting.md`
- `04-rune-cannon-damage.md`
