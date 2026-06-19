# Relic economy & match scoring

Status: ready-for-agent
Type: AFK
User stories: 15, 20

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

The economic spine of a match: collecting relics and banking them to win. End-to-end through
pickup → carry → bank → score → match end:

- **Relics** populate the Drift (count scaling with N at constant density). A ship picks up relics
  up to a **carry cap**.
- Banking carried relics at the ship's **Anchor** adds their value to that ship's score.
- A match is **time-boxed**: it ends after a fixed number of ticks (default 3600), and the highest
  score wins. The end produces a decisive result.
- Relic value, carry cap, relic count, and match length all come from the parameter set.

Pickup also granting a Sigil is handled in the Sigil-framework slice; this slice is purely the
relic/score economy. Kill bounty contributing to score arrives with the destruction slice.

## Acceptance criteria

- [ ] Relics spawn in the Drift, with population scaling with ship count N.
- [ ] A ship picks up relics up to the carry cap and cannot exceed it.
- [ ] Banking at the Anchor moves carried relic value into the ship's score and clears what was
      carried.
- [ ] A match ends after the configured tick count and reports the highest-scoring ship as winner.
- [ ] Golden scenario: a relic picked up and carried to the Anchor scores the expected value.
- [ ] Golden scenario: a full match is decisive with no shutout.

## Blocked by

- `01-engine-skeleton-determinism.md`
- `02-drift-movement-piloting.md`
