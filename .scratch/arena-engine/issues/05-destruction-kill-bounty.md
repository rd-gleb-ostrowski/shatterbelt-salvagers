# Destruction & kill bounty

Status: ready-for-agent
Type: AFK
User stories: 13 (destruction), 19

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

Close the combat loop: a ship whose **Hull** reaches zero is **destroyed**, and the responsible
ship is rewarded. End-to-end through lethal hit → destruction event → score:

- When a ship's Hull reaches zero it is destroyed and removed from active play (the post-death
  consequences — relic drop, respawn, spawn-protection — are the next slice).
- A kill awards a direct **score bounty** to the killer (no extra relic drop), so denial is worth
  pursuing.
- Destruction surfaces as a per-ship event identifying victim and killer, and the killer's score
  Observation reflects the bounty.

Bounty value comes from the parameter set.

## Acceptance criteria

- [ ] A ship whose Hull reaches zero is marked destroyed and leaves active play.
- [ ] The killer receives the configured score bounty; an environmental/self death awards no
      bounty.
- [ ] A destruction event is emitted identifying victim and (if any) killer.
- [ ] Banked-relic score and kill bounty combine into the ship's total score used for the match
      result.
- [ ] Golden scenario: a ship destroyed by cannon fire awards its killer the bounty and the kill
      registers in the match outcome.

## Blocked by

- `04-rune-cannon-damage.md`
