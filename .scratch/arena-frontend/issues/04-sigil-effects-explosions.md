# Sigil effects & explosions

Status: ready-for-agent
Type: AFK
User stories: 4

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

Render the big plays so they read clearly on the projector, using the observer god-mode stream
(which sees everything, including otherwise-hidden mines).

- **Sigil effects**: singularity gravity wells, deployed Aether Mines, and Arc Lance beams.
- **Rune-cannon bolts** in flight.
- **Explosions** when a ship is destroyed.

Effects are driven by the stream's entities and `events`, layered onto the core Viewer render.

## Acceptance criteria

- [ ] Singularity wells, Aether Mines, and Arc Lance beams render distinctly when present in the
      stream.
- [ ] Rune-cannon bolts are drawn travelling through the Drift.
- [ ] A ship destruction produces a clear explosion effect.
- [ ] Mines render even though they are hidden from bots (the observer stream exposes them).

## Blocked by

- `01-viewer-observer-render.md`
