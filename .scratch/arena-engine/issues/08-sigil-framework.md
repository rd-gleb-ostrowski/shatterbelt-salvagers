# Sigil framework

Status: ready-for-agent
Type: AFK
User stories: 16

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

The plumbing for single-use **Sigils**, with no specific effects yet. End-to-end through
relic pickup → grant → hold → discharge Intent → consume:

- Picking up a relic grants the ship a **single random Sigil** *only if it currently holds none*.
- A ship holds **at most one** Sigil at a time; the held Sigil is visible in the ship's own
  Observation (and hidden from enemies, per `PROTOCOL.md`).
- A discharge Intent consumes the held Sigil; with none held it is a no-op.
- Sigil grant/discharge surface as per-ship events.

This slice establishes the framework with a placeholder/no-op effect so the grant→hold→discharge
lifecycle is fully testable. The five concrete effects land in the two following slices. Sigil
drop rate / random selection is driven by the seeded RNG and parameter set for determinism.

## Acceptance criteria

- [ ] Picking up a relic grants exactly one random Sigil when the ship holds none.
- [ ] Picking up a relic while already holding a Sigil grants no additional Sigil.
- [ ] A ship can hold at most one Sigil; the held Sigil appears in its own Observation only.
- [ ] Discharging consumes the held Sigil and emits an event; discharging with none held is a
      no-op.
- [ ] Random Sigil selection is deterministic for a fixed seed.

## Blocked by

- `03-relic-economy-scoring.md`
