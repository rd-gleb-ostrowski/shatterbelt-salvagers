# Bare-minimal WS starter (no abstraction)

Status: ready-for-agent
Type: AFK
User stories: 7, 10, 13, 15 (minimal reference)

## Parent

`.scratch/arena-bot-sdks/PRD.md`

## What to build

A standalone, **bare-minimal WS Bot** project that hides nothing — the rawest possible functional
reference for the WebSocket path against `src/arena/PROTOCOL.md`. No SDK, no helper library, no
codegen: just the minimum code to connect, handshake, and exchange JSON.

- A single small project that opens a WebSocket to the Arena Server and performs the raw handshake
  inline: `welcome` → `join`(token) → `assigned` → `matchStart` → loop `tick`/`action` → `matchEnd`.
- Reads the observation JSON each tick and writes an action JSON, with the parsing/serialisation
  written out plainly (no abstraction over the protocol).
- Registers with the shared event password to obtain a token (or accepts a pre-issued token via one
  config value).
- A trivial inline decision (e.g. constant thrust / occasional fire) — just enough to be a
  functional, readable baseline, deliberately *not* the shared heuristic.
- Build/run/submit instructions front-and-centre.

This is intentionally separate from the paved-road Rust template: its value is being a transparent,
copy-and-read reference for understanding the wire protocol with zero magic.

## Acceptance criteria

- [ ] A self-contained project connects over WebSocket and completes the full handshake to play a
      match.
- [ ] Observation JSON is read and action JSON is written each tick with no abstraction layer over
      the protocol.
- [ ] It registers with the event password (or accepts a token via config) to join.
- [ ] Run through a match it emits schema-valid actions each tick and plays to match end.
- [ ] README explains build/run/submit and that this is the minimal no-magic reference.

## Blocked by

None - can start immediately.
