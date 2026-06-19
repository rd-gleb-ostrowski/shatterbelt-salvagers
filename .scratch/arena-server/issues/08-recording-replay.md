# Recording & replay

Status: ready-for-agent
Type: AFK
User stories: 17, 18

## Parent

`.scratch/arena-server/PRD.md`

## What to build

Persist every match so any of them can be re-shown deterministically.

- For every match, **record the seed and the applied-intent log**.
- **List** recorded matches.
- **Replay** a recorded match by re-running its seed + applied-intent log through the engine,
  reconstructing an identical result (the engine is deterministic).
- Replay playback is a server-owned timing mode (alongside live and headless).

TrueSkill and the Admin UI are out of scope here; this slice is purely record + list + replay.

## Acceptance criteria

- [ ] Every finished match has its seed and applied-intent log stored.
- [ ] Recorded matches can be listed.
- [ ] Replaying a recorded match reconstructs an identical match state and result via the engine.
- [ ] Replay is played back through the server's timing layer (not just recomputed silently) so it
      can drive the observer stream.
- [ ] Replay seam test: a recorded match replays to an identical result.

## Blocked by

- `01-server-skeleton-live-loop.md`
