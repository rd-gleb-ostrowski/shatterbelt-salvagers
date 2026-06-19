# Respawn, relic-drop & spawn-protection

Status: ready-for-agent
Type: AFK
User stories: 13 (drop/respawn), 14

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

Make death a setback, not an elimination. End-to-end through destruction → drop → respawn →
invulnerable window:

- A destroyed ship **drops the relics it was carrying** back into the Drift (recoverable by
  others).
- After a **respawn delay** the ship returns at its **Anchor**.
- A freshly respawned ship has a **spawn-protection** window of invulnerability so it can't be
  spawn-camped; an `invuln` flag is exposed on the ship in Observations.
- While invulnerable the ship takes no damage from any source; the window expires after its
  configured duration.

Respawn delay, spawn-protection duration, and drop behaviour come from the parameter set.

## Acceptance criteria

- [ ] A destroyed ship drops its carried relics into the Drift where others can pick them up.
- [ ] The ship respawns at its Anchor after the configured delay.
- [ ] A respawned ship is invulnerable for the spawn-protection window and the `invuln` flag is
      visible in Observations.
- [ ] Damage from cannon fire, collisions, mines, etc. is fully blocked while invulnerable.
- [ ] The invulnerability expires after its configured duration and normal damage resumes.
- [ ] Golden scenario: a destroyed ship drops its relics and respawns invulnerable.
- [ ] Golden scenario: spawn-protection blocks damage for its window.

## Blocked by

- `03-relic-economy-scoring.md`
- `05-destruction-kill-bounty.md`
