# Headless balance harness & replay parity

Status: ready-for-agent
Type: AFK
User stories: 23 (+ 6, 22 at full scale)

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

The evidence-gathering and replay layer over a fully-featured engine. Two capabilities:

- **Headless harness**: run many full matches with scripted/stub bots and read **aggregate stats**
  (e.g. TTK, score spread, relic flow, kill counts) so balance can be validated from evidence. This
  mirrors `src/arena/balance/harness.py`; reproduce its scenarios and expected magnitudes as Rust
  tests.
- **Replay parity**: combined with the seed and rapier determinism, the recorded applied-Intent log
  reproduces a full match **exactly** (state and result) across runs — the determinism guarantee
  proven at full gameplay scale, not just the empty-intent skeleton.

TrueSkill, persistence, and storage stay out of the engine — this is purely in-engine headless
running and replay.

## Acceptance criteria

- [ ] A headless runner executes many matches with scripted/stub bots and emits aggregate stats.
- [ ] The aggregate magnitudes mirror those encoded in `src/arena/balance/harness.py`.
- [ ] Replaying a recorded seed + applied-Intent log reproduces an identical match state and result.
- [ ] A full real-gameplay match (movement, combat, economy, collisions, sigils) is deterministic
      across runs.
- [ ] Golden scenario: a representative full match is decisive with no shutout under the harness.

## Blocked by

- `02-drift-movement-piloting.md`
- `03-relic-economy-scoring.md`
- `05-destruction-kill-bounty.md`
- `06-respawn-spawn-protection.md`
- `07-collisions-hazards.md`
- `09-self-buff-sigils.md`
- `10-world-effect-sigils.md`
