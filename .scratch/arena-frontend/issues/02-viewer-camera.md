# Viewer camera: fit, follow, zoom & pan

Status: ready-for-agent
Type: AFK
User stories: 10, 11

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

A flexible camera over the Viewer render so operators can frame the action.

- **Fit-the-Drift** default so nothing happens off-screen (the Drift scales with field size, so the
  fit must recompute per match).
- **Follow a chosen ship** so a duel can be highlighted.
- **Zoom in/out** and **pan**.
- Camera transforms are pure world↔screen functions (fit/zoom/pan/follow), kept framework-free and
  unit-tested per the PRD's testing decisions.

## Acceptance criteria

- [ ] The default camera fits the whole Drift, recomputed for the current match's Drift size.
- [ ] The operator can select a ship to follow and the camera tracks it.
- [ ] Zoom in/out and pan work and compose correctly with follow/fit.
- [ ] Camera transforms (world↔screen, fit, zoom, pan) are framework-free functions with unit
      tests.

## Blocked by

- `01-viewer-observer-render.md`
