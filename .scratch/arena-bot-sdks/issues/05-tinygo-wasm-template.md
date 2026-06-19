# TinyGo WASM template

Status: ready-for-agent
Type: AFK
User stories: 4

## Parent

`.scratch/arena-bot-sdks/PRD.md`

## What to build

A WASM Bot template in **TinyGo** so Go developers can compete via upload, with the core-wasm glue
hidden (ADR-0004) — the author implements only `tick(observation) -> action` (and optionally `init`)
against typed `Observation`/`Action`.

Reuses the shared sample-bot heuristic and the play-test/schema harness from the Rust paved-road
slice. Ships the sample bot as a working baseline with build/submit docs.

## Acceptance criteria

- [ ] A TinyGo author implements only `tick`/`init`; the template hides the core-wasm glue.
- [ ] The template compiles to a valid `.wasm` artifact.
- [ ] `init` receives the tick-0 observation.
- [ ] The shared sample bot is included and, run through the harness, emits schema-valid actions and
      scores at least one relic uncontested.
- [ ] Observation/action schema round-trip is tested; build/submit docs are included.

## Blocked by

- `03-rust-paved-road-template.md`
