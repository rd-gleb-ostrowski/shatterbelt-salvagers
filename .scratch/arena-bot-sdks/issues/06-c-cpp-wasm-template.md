# C/C++ WASM template (clang/wasi-sdk)

Status: ready-for-agent
Type: AFK
User stories: 5

## Parent

`.scratch/arena-bot-sdks/PRD.md`

## What to build

A WASM Bot template in **C/C++** via clang/wasi-sdk so systems programmers can compete, with the
core-wasm glue hidden (ADR-0004) — the author implements only `tick(observation) -> action` (and
optionally `init`) against typed `Observation`/`Action`.

Reuses the shared sample-bot heuristic and the play-test/schema harness from the Rust paved-road
slice. Ships the sample bot as a working baseline with build/submit docs.

## Acceptance criteria

- [ ] A C/C++ author implements only `tick`/`init`; the template hides the core-wasm glue.
- [ ] The template compiles/links via clang/wasi-sdk to a valid `.wasm` artifact.
- [ ] `init` receives the tick-0 observation.
- [ ] The shared sample bot is included and, run through the harness, emits schema-valid actions and
      scores at least one relic uncontested.
- [ ] Observation/action schema round-trip is tested; build/submit docs are included.

## Blocked by

- `03-rust-paved-road-template.md`
