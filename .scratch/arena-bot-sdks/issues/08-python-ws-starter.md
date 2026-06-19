# Python WS starter

Status: ready-for-agent
Type: AFK
User stories: 7, 10

## Parent

`.scratch/arena-bot-sdks/PRD.md`

## What to build

A **Python** WS starter (Python is WS-only — no practical core-wasm target) so participants can
write a bot with no WASM toolchain. It connects over WebSocket, performs the welcome → join(token)
→ assigned handshake, then loops reading `tick` observation JSON and writing `action` intent JSON,
exposing a tiny `tick(observation) -> action` surface.

Reuses the shared sample-bot heuristic and the play-test/schema harness from the Rust paved-road
slice. Registers with the event password for a token (or accepts one via config). Ships the sample
bot with build/run/submit docs.

## Acceptance criteria

- [ ] The starter connects over WebSocket and completes the welcome → join(token) → assigned
      handshake.
- [ ] The author implements a small `tick(obs) -> action`; the starter handles the connection loop
      and JSON.
- [ ] It registers with the event password (or accepts a token via config) to join.
- [ ] The shared sample bot is included and, run through the harness, emits schema-valid actions and
      scores at least one relic uncontested.
- [ ] Observation/action schema round-trip is tested; run/submit docs are included.

## Blocked by

- `03-rust-paved-road-template.md`
