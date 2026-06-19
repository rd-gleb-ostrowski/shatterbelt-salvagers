# Viewer replay mode

Status: ready-for-agent
Type: AFK
User stories: 12

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

Let spectators re-watch and slow-mo great moments in the Viewer.

- Load a **recorded match** from the server (seed + applied-intent log, reconstructed via the
  server/engine into the same observer stream the live Viewer renders).
- Playback controls: **play/pause**, **speed** control, and a **scrubbable timeline**.
- Reuses the live Viewer's render path so replays look identical to live matches.

The replay timeline/scrub mapping (tick↔timeline position, speed) is a framework-free function with
unit tests, per the PRD.

## Acceptance criteria

- [ ] A recorded match can be loaded and rendered through the same Viewer render path as a live
      match.
- [ ] Play/pause, speed control, and a scrubbable timeline all work.
- [ ] Scrubbing to a position shows the correct tick; speed changes play back faster/slower.
- [ ] The tick↔timeline mapping and speed logic are framework-free functions with unit tests.

## Blocked by

- `01-viewer-observer-render.md`
