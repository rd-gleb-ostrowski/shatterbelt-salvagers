# Ship presentation & state cues

Status: ready-for-agent
Type: AFK
User stories: 2, 3

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

Make each ship legible so spectators can tell who's who and how they're doing.

- Render ships in **team colours** with their **name**, current **heading**, and **thrust flames**
  when thrusting.
- Show **hull and shield bars** per ship.
- Show a visible **shimmer** on ships during **spawn protection** (the `invuln` flag from the
  stream) so it's clear why they aren't taking damage.

These cues are driven by the per-tick observer stream rendered in the core Viewer slice.

## Acceptance criteria

- [ ] Ships render in distinct team colours with their team name shown.
- [ ] Ship heading is visible and thrust flames appear when a ship is thrusting.
- [ ] Each ship shows hull and shield bars reflecting current values.
- [ ] A ship with the `invuln` flag set shows a spawn-protection shimmer; it disappears when the
      flag clears.

## Blocked by

- `01-viewer-observer-render.md`
