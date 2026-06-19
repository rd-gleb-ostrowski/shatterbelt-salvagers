# Rust paved-road template & shared harness

Status: ready-for-agent
Type: AFK
User stories: 1, 2, 10, 11, 12, 13, 14, 15

## Parent

`.scratch/arena-bot-sdks/PRD.md`

## What to build

The **paved-road** Rust template and the shared scaffolding every other template reuses. The author
implements one decision function and the template handles everything else.

- A Rust template where the author implements only `tick(observation) -> action` (and optionally
  `init`), with a **macro/codegen hiding the core-wasm glue** (`memory`/`alloc`/`init`/`tick`,
  packed ptr/len, JSON in linear memory) — they never touch a pointer.
- The same decision function **compiles to both a WASM Bot module and a WS Bot binary** (the WS
  binary does the welcome → join(token) → assigned handshake and per-tick JSON loop), so a bot runs
  live or uploaded without rewriting.
- A **shared sample-bot heuristic** (seek nearest relic, bank at carry cap, fire when an enemy is
  roughly ahead) shipped as the starting point — defined here as the canonical spec the other
  templates reuse.
- A **play-test + schema round-trip harness** that drives a sample bot through a headless match
  (against the Arena Engine/Server or a stub) and asserts schema-valid actions and a basic outcome
  — reused by all subsequent template slices.
- Auto-registration: calls `POST /register` with the event password for a token (or accepts a
  pre-issued token via config).
- Build/submit docs front-and-centre; the bot is a single artifact / small repo for an easy
  brownfield swap.

## Acceptance criteria

- [ ] An author implements only `tick(obs) -> action` (and optionally `init`); the macro handles all
      core-wasm glue.
- [ ] The same decision function builds to a valid `.wasm` module *and* a runnable WS binary.
- [ ] `init` receives the tick-0 observation in the WASM build.
- [ ] The shared sample heuristic is implemented and documented as the reusable spec.
- [ ] The play-test harness runs the sample bot through a headless match asserting schema-valid
      actions and that it scores at least one relic in an uncontested run.
- [ ] Observation/action schema round-trip (decode/encode) is tested against the protocol.
- [ ] The template registers for a token automatically (or via one config value) and documents
      build/submit.

## Blocked by

None - can start immediately.
