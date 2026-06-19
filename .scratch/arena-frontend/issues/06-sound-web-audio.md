# Sound (Web Audio)

Status: ready-for-agent
Type: AFK
User stories: 8, 9

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

Event-driven sound so the spectacle has impact — a first-class part of the Viewer.

- Map the observer stream's `events` to **Web Audio** sounds: thrust, cannon fire, explosions,
  Sigil discharges, and relic pickup/bank.
- **Match start/end stingers** so matches feel like events.
- The event→sound mapping is a pure, framework-free function (event in → sound cue out),
  unit-tested per the PRD.

## Acceptance criteria

- [ ] Stream events trigger the correct sounds for thrust, cannon fire, explosions, Sigil
      discharges, and relic pickup/bank.
- [ ] Match start and end play distinct stingers.
- [ ] The event→sound mapping is a framework-free function with unit tests.
- [ ] Audio is driven by the live stream without blocking or desyncing the render.

## Blocked by

- `01-viewer-observer-render.md`
