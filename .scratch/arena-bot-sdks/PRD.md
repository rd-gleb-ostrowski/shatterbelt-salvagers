# Bot SDKs & Starters — Shatterbelt Salvagers

Status: ready-for-agent

## Problem Statement

Participants — many of them beginners using coding agents — need to get a working bot into the
Arena *fast*, in a language they're comfortable with, without wrestling raw WebSocket framing or
the low-level WASM ABI (pointers, packed return values, manual JSON in linear memory). If the
on-ramp is hard, the competition is inaccessible. We need ready-made templates that reduce
"write a bot" to "implement one function."

## Solution

Two families of starter templates, both built against the single JSON contract in
`src/arena/PROTOCOL.md`:

- **WASM SDK templates** (Rust, AssemblyScript, TinyGo, C/C++, Zig) that hide the core-wasm glue
  so the author just implements `tick(observation) -> action` (and optionally `init`).
- **WS starter templates** (Python, TypeScript on Deno, Kotlin on the JVM) that connect, register
  with a token, read the observation JSON each tick, and write an action JSON — the accessible
  path for any language.

Each template ships a trivial but functional sample bot so a participant can build, submit, and
see it play within minutes, then iterate.

## User Stories

1. As a Rust participant, I want a WASM template where I implement `tick(obs) -> action` and the macro/template handles `memory`/`alloc`/`init`/`tick` glue, so that I never touch a pointer.
2. As a Rust participant, I want to build my one decision function as *both* a WASM Bot and a WS Bot binary, so that I can run it live or upload it without rewriting.
3. As an AssemblyScript participant, I want a WASM template with the glue hidden, so that the JS/TS crowd can write a bot in a familiar language.
4. As a TinyGo participant, I want a WASM template, so that Go developers can compete via upload.
5. As a C/C++ participant, I want a WASM template via clang/wasi-sdk, so that systems programmers can compete.
6. As a Zig participant, I want a WASM template, so that Zig developers can compete.
7. As a Python participant, I want a WS starter that connects, registers, and exchanges JSON, so that I can write a bot with no WASM toolchain.
8. As a TypeScript/Deno participant, I want a WS starter, so that I can write a bot in TypeScript on a modern runtime.
9. As a Kotlin participant, I want a WS starter on the JVM, so that I can use Kotlin (since Kotlin/Wasm isn't supported in v1).
10. As any participant, I want my template to register with the shared event password and obtain a token automatically (or with one config value), so that joining is simple.
11. As a WASM author, I want my `init` to receive the tick-0 observation, so that I can precompute before the match.
12. As a participant, I want a sample bot in each template that already pilots toward relics and fires opportunistically, so that I have a working baseline to improve.
13. As a participant, I want each template documented with how to build and submit, so that I can go from clone to competing quickly.
14. As a participant handing my project to another team in the afternoon, I want my bot to be a single artifact or a small repo, so that the brownfield swap is easy.
15. As a facilitator, I want the templates to validate against the live protocol, so that "it builds and connects" is a real guarantee on the day.

## Implementation Decisions

- **Contract:** every template targets the JSON observation/action schema in
  `src/arena/PROTOCOL.md`; a bot is "implement `tick(obs) -> action`".
- **WASM ABI (ADR-0004):** core-wasm — exports `memory`, `alloc(len)`, `init(ptr,len)` (tick-0
  observation), and `tick(ptr,len) -> packed (out_ptr<<32 | out_len)`. The template hides this
  behind a macro/codegen so the author sees only typed `Observation`/`Action`. Instance persists
  across ticks; per-tick fuel budget applies (author writes ordinary code).
- **Rust as the paved road:** the Rust template factors the decision function so the same code
  compiles to a WASM module *and* a WS binary.
- **WS starters:** connect over WebSocket, perform the welcome → join(token) → assigned handshake,
  then loop reading `tick` observations and writing `action` intents; Python, Deno-TS, Kotlin-JVM.
- **Registration:** templates call `POST /register` with the event password to obtain a token (or
  accept a pre-issued token via config).
- **Kotlin/Wasm** is an experimental stretch only; Kotlin users take the WS path in v1. Python is
  WS-only (no practical core-wasm target).
- **Sample bot:** a shared simple heuristic (seek nearest relic, bank at carry cap, fire when an
  enemy is roughly ahead) provided in each template as a starting point.

## Testing Decisions

- Good tests assert the template's **observable contract**: it builds, and a sample bot, run
  through a match, **produces well-formed actions** and behaves (e.g. scores at least one relic in
  an uncontested run).
- **Per-template build test:** each template compiles/links (WASM modules to valid `.wasm`; WS
  starters run).
- **Play test:** drive each sample bot through a headless match (against the Arena Engine/Server
  or a stub harness) and assert it emits schema-valid actions each tick and reaches a basic
  outcome.
- **Schema round-trip:** observations decode and actions encode against the protocol schema.
- Prior art: the Arena Engine and Server seams provide the match/harness to run sample bots in.

## Out of Scope

The Arena Engine and Server internals, the Viewer/Admin, advanced bot AI (participants write
their own strategy), and any language beyond the listed set in v1.

## Further Notes

The whole point is the on-ramp: a beginner with a coding agent should go from template to a
submitted, playing bot in minutes. Keep each template's "implement `tick`" surface tiny and the
build/submit instructions front-and-centre.
