# HUD: scoreboard, timer & ladder panel

Status: ready-for-agent
Type: AFK
User stories: 6, 7

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

The on-screen HUD that makes the state of play and the standings legible.

- **Live scoreboard**: per team, banked score and relics currently carried.
- **Match timer** (the ~2-minute match countdown).
- **TrueSkill ladder panel** shown between matches so spectators can follow the standings.

Scoreboard and timer are driven by the observer stream; the ladder panel reads the server's
standings. HUD formatting logic (scores, timer) is framework-free and unit-tested.

## Acceptance criteria

- [ ] A scoreboard shows each team's banked score and relics carried, updating live.
- [ ] A match timer shows the remaining match time.
- [ ] A TrueSkill ladder panel displays standings between matches.
- [ ] Score/timer formatting are framework-free functions with unit tests.

## Blocked by

- `01-viewer-observer-render.md`
