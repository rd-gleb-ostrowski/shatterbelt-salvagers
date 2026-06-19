# Self-buff Sigils: Afterburner & Bulwark

Status: ready-for-agent
Type: AFK
User stories: 17 (self-buffs)

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

The two self-targeting **Sigil** effects, riding on the Sigil framework. End-to-end through
discharge → timed self-buff → Observation/events:

- **Afterburner**: a sustained directional thrust/speed surge for a timed window, free of aether
  cost; it keeps inertia and must still be steered (it does not snap heading).
- **Bulwark**: instantly overcharges/refills the **Shield** and grants a few ticks of damage
  immunity.
- Both are timed effects that expire on their own; their active state is visible in the ship's own
  Observation, and activation/expiry surface as per-ship events.

Boost magnitudes and durations come from the parameter set.

## Acceptance criteria

- [ ] Discharging Afterburner raises the ship's thrust/speed envelope for its timed window without
      consuming aether, then reverts.
- [ ] Afterburner preserves inertia (no instantaneous heading snap); steering still applies.
- [ ] Discharging Bulwark refills/overcharges the Shield and grants damage immunity for its window.
- [ ] During Bulwark immunity the ship takes no damage; immunity expires after its duration.
- [ ] Active self-buff state is visible in the ship's own Observation and emits activation/expiry
      events.
- [ ] Golden scenario: Afterburner produces the expected speed-up; Bulwark produces the expected
      immunity.

## Blocked by

- `08-sigil-framework.md`
- `04-rune-cannon-damage.md`
