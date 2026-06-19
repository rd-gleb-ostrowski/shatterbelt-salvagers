# Engine skeleton & determinism

Status: ready-for-agent
Type: AFK
User stories: 1, 2, 3, 4, 5, 6, 7, 22

## Parent

`.scratch/arena-engine/PRD.md`

## What to build

The end-to-end skeleton of the pure Rust **engine** that everything else hangs off. The engine is
constructed from a seed, a parameter set, and a field of salvage ships, and exposes the full tick
loop with no gameplay rules yet beyond bookkeeping:

- Construct an engine from `(seed, params, ships)` with a known starting state.
- `step(intents) -> events`: advance exactly one tick, taking each ship's Intent and returning the
  per-ship events that occurred this tick (empty for now beyond lifecycle/no-op).
- A per-ship **Observation** accessor that produces what a single bot would see.
- A god-mode **Viewer** view of the whole world for the projector/recording.
- A **parameter set** type that holds all gameplay numbers, seeded from the first-pass values in
  `src/arena/balance/params.py` / `BALANCE.md` (placeholders, tunable).
- The engine **records the applied Intent per ship per tick** so a match can later be replayed.

The engine has no awareness of WebSocket, WASM, HTTP, the Viewer client, the Ladder, or auth. This
slice proves the pipeline (construct → step → observe → god-view) is wired and deterministic before
any real physics or rules land. Observation and Intent shapes must match `src/arena/PROTOCOL.md`
(§6 Observation, §8 Action); if they drift, update PROTOCOL.md in the same change.

## Acceptance criteria

- [ ] Engine constructs from a seed, a parameter set, and a list of ships into a known state.
- [ ] `step(intents)` advances one tick and returns a per-ship events collection.
- [ ] A per-ship Observation can be produced, matching the `PROTOCOL.md` §6 shape.
- [ ] A god-mode Viewer view of the full world can be produced (`PROTOCOL.md`-consistent).
- [ ] All gameplay numbers come from a parameter set type, seeded from the first-pass values.
- [ ] Applied Intents are recorded per ship per tick.
- [ ] Determinism test: the same seed + the same empty-intent sequence reproduces an identical
      match state across runs.
- [ ] Engine has zero dependencies on networking, WASM, HTTP, ladder, or auth.

## Blocked by

None - can start immediately.
