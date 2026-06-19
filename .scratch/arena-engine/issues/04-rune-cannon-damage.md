# Rune-cannon & shield/hull damage

Status: ready-for-agent
Type: AFK
User stories: 11, 12

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

Give ships their standard weapon and a damage model. End-to-end through fire Intent → projectile →
hit → Shield/Hull → Observation/events:

- A ship fires a fixed **rune-cannon** along its heading, subject to a **cooldown** and an
  **aether cost**, producing arcane projectiles that travel and can hit other ships.
- Damage hits the **Shield** first, then the **Hull**. The Shield **regenerates** after a delay
  during which the ship takes no hit.
- Hits surface as per-ship events (so the Server can forward them to bots and the Viewer), and the
  shooter/target Observations reflect the new Shield/Hull/aether values.

Destruction (Hull reaching zero) and the kill bounty are the next slice; this slice stops at
applying damage and regenerating shields. Cannon damage, cooldown, aether cost, projectile speed,
and shield-regen delay/rate all come from the parameter set.

## Acceptance criteria

- [ ] Firing emits a projectile along heading only when off-cooldown and with enough aether; it
      deducts the aether cost and starts the cooldown.
- [ ] Firing on cooldown or without aether produces no projectile and no cost.
- [ ] Projectile hits deal damage to the target's Shield first, then Hull.
- [ ] Shield regenerates after the configured no-hit delay and stops at its cap.
- [ ] Hits produce per-ship events and are reflected in the relevant Observations.
- [ ] Golden scenario: N cannon hits reduce a ship's Hull to zero in the expected time-to-kill
      (mirroring the Python harness's TTK magnitude).

## Blocked by

- `01-engine-skeleton-determinism.md`
- `02-drift-movement-piloting.md`
