# Bare-minimal WASM starter (raw core-wasm ABI)

Status: ready-for-agent
Type: AFK
User stories: 11, 13, 14 (minimal reference)

## Parent

`.scratch/arena-bot-sdks/PRD.md`

## What to build

A standalone, **bare-minimal WASM Bot** project that hides nothing — the rawest possible functional
reference for the upload path and the core-wasm ABI (ADR-0004). No macro, no codegen, no typed
wrapper: the author sees the pointers.

- A single small project that exports the raw core-wasm ABI directly: `memory`, `alloc(len)`,
  `init(ptr,len)` (tick-0 observation), and `tick(ptr,len) -> packed (out_ptr<<32 | out_len)`.
- Reads the observation JSON out of linear memory and writes the action JSON back into memory by
  hand, returning the packed pointer/length — all written out plainly so the ABI is fully visible.
- A trivial inline decision (e.g. constant thrust / occasional fire), deliberately *not* the shared
  heuristic.
- Compiles to a valid `.wasm` artifact; build/submit instructions front-and-centre.

This is intentionally separate from the paved-road Rust template (which hides this glue): its value
is being a transparent reference for exactly what the WASM ABI demands.

## Acceptance criteria

- [ ] A self-contained project compiles to a valid `.wasm` exporting `memory`/`alloc`/`init`/`tick`
      per the core-wasm ABI.
- [ ] Observation JSON is read from and action JSON is written to linear memory by hand, returning
      the packed ptr/len, with no wrapper hiding the ABI.
- [ ] `init` receives the tick-0 observation.
- [ ] Run through a headless match it emits schema-valid actions each tick within the fuel budget.
- [ ] README explains build/submit and that this is the minimal no-magic ABI reference.

## Blocked by

None - can start immediately.
