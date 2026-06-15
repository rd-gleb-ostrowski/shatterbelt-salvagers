# WASM bot ABI: minimal core-wasm with multi-language SDKs

Uploaded **WASM Bots** run in-process in the Arena (via wasmtime) against the **same** JSON
observation/action contract as WS Bots (see ADR-0003). The interface is a **minimal core-wasm
ABI**, not the Component Model, so the widest range of languages can target it; an SDK/template
per language hides the raw glue so authors just write `tick(observation) -> action`.

## The ABI (core-wasm)

A bot module exports:
- `memory` — its linear memory.
- `alloc(len: i32) -> i32` — the host allocates a buffer in the module and writes JSON into it.
- `init(ptr: i32, len: i32)` — called once before the match with the **tick-0 observation**
  (arena size, the bot's ship id and `class`, anchor positions, opening layout, `seed`), so the
  bot can precompute and allocate however it likes.
- `tick(ptr: i32, len: i32) -> i64` — reads the observation JSON at `[ptr,len)`, writes its action
  JSON into its own memory, and returns a packed `i64` = `(out_ptr << 32) | out_len`.

The instance **persists for the whole match**, so the bot keeps state in its own memory/globals.
The only optional host import is `log(ptr, len)` for debug output to the viewer.

## Limits & fairness

- Per-tick compute is bounded with wasmtime **fuel** (an instruction budget), not epoch/wall-clock,
  so the limit is reproducible and independent of host load. Blowing the budget = "no action this
  tick", exactly like a late WS Bot.
- Modules are **instantiated and `init`-ed before the match** to pay any JIT cost up front, not on
  the first tick.

## Languages

- **WASM SDKs (v1):** Rust (paved road, shares code with the WS binary), AssemblyScript, TinyGo,
  C/C++, Zig — each with a glue-hiding template.
- **WS starters (v1):** Python, TypeScript on **Deno**, and Kotlin (JVM).
- **Kotlin/Wasm** is an experimental stretch goal only (it targets WasmGC and is browser-oriented;
  driving it from a wasmtime host is immature). Kotlin users take the WS path in v1.

## Considered alternatives

- **WASM Component Model (WIT)** — nicer raw DX (no manual alloc), but heavier, less universal
  tooling; and since we pass JSON strings (to keep one schema), its typed-record benefit goes
  largely unused while our SDK already hides the alloc glue.
- **Rust-only WASM** — simpler for us, but we want WASM authoring open to more languages.
- **Epoch/wall-clock limiting** — rejected in favour of fuel for reproducible, fair budgets.
