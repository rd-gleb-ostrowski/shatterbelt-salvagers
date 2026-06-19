# Drift movement & piloting

Status: ready-for-agent
Type: AFK
User stories: 8, 9, 10, 21

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

Make salvage ships actually fly through the Drift under rate-first control. Cutting through Intent →
physics → Observation:

- Continuous 2D drift movement via `rapier2d` in deterministic mode: thrust applies force/momentum
  along heading, with light linear damping and a max-speed cap, so piloting feels like space but
  stays controllable.
- **Rate-first** control only: a ship sets a turn rate and a thrust fraction; the engine applies
  the physics and clamps. A bot never sets absolute position/velocity/heading. Omitted Intent
  fields persist their previous value.
- Thrust costs **aether**; aether regenerates over time, so moving trades against fighting later.
- **Dynamic Drift**: arena dimensions scale by `√(N/4)` off the 2000×1200 baseline (area ∝ N) so
  2-ship and 8-ship matches feel equally dense. (Asteroid/relic population scaling lands in the
  slices that introduce those entities.)

All magnitudes come from the parameter set.

## Acceptance criteria

- [ ] A ship under sustained thrust accelerates along its heading and is capped at max speed.
- [ ] Turn is rate-controlled; thrust is a fraction; absolute state can never be set by a bot.
- [ ] Omitted Intent fields retain their previous tick's value.
- [ ] Thrust deducts aether; aether regenerates per the parameter set; thrust at zero aether is
      ineffective.
- [ ] Light damping brings an un-thrusting ship's speed down over time.
- [ ] Drift dimensions scale by `√(N/4)` off the 2000×1200 baseline with ship count N.
- [ ] Golden scenario: a thrusting ship reaches the expected speed/position envelope (mirroring the
      Python harness's magnitudes).
- [ ] The run is deterministic for a fixed seed + applied-intent log.

## Blocked by

- `01-engine-skeleton-determinism.md`
