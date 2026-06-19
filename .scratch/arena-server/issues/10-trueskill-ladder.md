# TrueSkill ladder

Status: ready-for-agent
Type: AFK
User stories: 14, 15

## Parent

`.scratch/arena-server/PRD.md`

## What to build

A continuously-updated **Ladder** fed by finished matches.

- Each finished match's full **FFA ranking** is fed into **TrueSkill**, updating ratings with
  uncertainty so the ladder converges quickly.
- Both live and headless-fast matches feed the ladder (headless is what makes it churn).
- Standings can be **viewed** and **reset**.

This consumes the finished-match results produced by the live loop and the headless runner.

## Acceptance criteria

- [ ] A finished match's full FFA ranking updates each participating bot's TrueSkill rating.
- [ ] Ratings carry uncertainty and converge as more matches feed in.
- [ ] Both live and headless matches contribute to the ladder.
- [ ] Standings can be retrieved and reset.
- [ ] Ladder seam test: feeding a known match ranking produces the expected rating movement.

## Blocked by

- `09-headless-fast-runner.md`
