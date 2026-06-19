# World-effect Sigils: Singularity, Aether Mine & Arc Lance

Status: ready-for-agent
Type: AFK
User stories: 17 (world-effects)

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

The three world-affecting **Sigil** effects, riding on the Sigil framework. End-to-end through
discharge → deployed/launched effect → impact on other entities → events/Observation:

- **Singularity**: deploys a short-lived gravity well at a point that draws loose **Relics** and
  enemy ships toward it, then expires.
- **Aether Mine**: drops a near-invisible proximity mine that arms and **detonates on enemy
  contact**, dealing area damage. Enemy mines are proximity-visible in Observations (per
  `PROTOCOL.md`); otherwise hidden.
- **Arc Lance**: fires one fast, **piercing** bolt that **bypasses Shields** and can hit multiple
  ships in a line.

Effects route damage/forces through the existing physics and Shield/Hull model (Arc Lance bypassing
Shield; collisions/mines respecting spawn-protection). Ranges, pull strength, durations, and damage
come from the parameter set.

## Acceptance criteria

- [ ] Singularity deploys a timed gravity well that pulls loose relics and enemy ships toward it
      and then disappears.
- [ ] Aether Mine deploys, arms, and detonates on enemy proximity, dealing area damage; it is
      proximity-visible to enemies and otherwise hidden.
- [ ] Arc Lance fires one fast bolt that pierces through Shields and can damage multiple ships along
      its line.
- [ ] All three respect spawn-protection invulnerability where applicable.
- [ ] Deployment/detonation/hit surface as per-ship events.
- [ ] Golden scenario: each of Singularity (pull), Aether Mine (detonation), and Arc Lance (pierce +
      shield bypass) produces its expected effect.

## Blocked by

- `08-sigil-framework.md`
- `04-rune-cannon-damage.md`
- `07-collisions-hazards.md`
