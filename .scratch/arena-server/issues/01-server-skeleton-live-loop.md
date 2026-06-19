# Server skeleton & live match loop

Status: ready-for-agent
Type: AFK
User stories: 6, 8 (partial), timing infrastructure

## Parent

`.scratch/arena-server/PRD.md`

## What to build

The axum server that wraps the Arena Engine and drives one match end-to-end with no network bots
yet. This establishes the live timing loop everything else plugs into:

- Boot an axum server that constructs an Arena Engine match and runs it to completion.
- **Live pacing** at ~30 Hz (ADR-0003): each tick has a per-tick **deadline**; the server collects
  intents, steps the engine, and advances.
- **Missed-deadline behaviour**: if no fresh intent arrives by the deadline, the ship's previous
  intent persists (no freeze).
- For this slice every slot is driven by the built-in **Default Bot** so a match can run with zero
  external connections; the full connection resolver arrives later.

This is the seam where server-owned timing meets the engine's pure stepper. Keep observation/intent
handling authoritative against `src/arena/PROTOCOL.md`.

## Acceptance criteria

- [ ] The server starts, constructs an engine match, runs it tick-by-tick, and reaches a decisive
      match end.
- [ ] Ticks are paced to ~30 Hz with a per-tick deadline owned by the server, not the engine.
- [ ] When a tick's deadline passes with no new intent for a ship, its previous intent persists.
- [ ] A match runs to completion with all slots filled by the Default Bot and no network present.
- [ ] Timing/deadline behaviour is tested against the engine through a stub transport (observable
      behaviour, not internals).

## Blocked by

None - can start immediately.
