# Minimal WASM Bot — Rust

The rawest functional reference for the **WASM** bot path and the core-wasm ABI
(ADR-0004 / [`PROTOCOL.md`](../../PROTOCOL.md) §9). No macro, no codegen, no
wrapper — `src/lib.rs` exports the ABI by hand and you see every pointer.

## What it shows

The module exports exactly:

- `memory` — our linear memory (a `cdylib` exports it automatically).
- `alloc(len) -> ptr` — the host asks us for a buffer and writes the observation
  JSON into it.
- `init(ptr, len)` — called once before the match with the tick-0 observation.
- `tick(ptr, len) -> i64` — read the observation at `[ptr, len)`, write an action
  JSON into our memory, and return the packed `(out_ptr << 32) | out_len`.

The instance persists for the whole match, so state lives in module globals
(`OUT`, `MY_ID`). `serde_json` parses the JSON; the ABI plumbing is explicit.
The per-tick decision is a trivial placeholder (steer at the nearest relic,
thrust, fire) — replace it; the ABI around it does not change.

## Build

Needs the `wasm32-unknown-unknown` target:

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
# artifact: target/wasm32-unknown-unknown/release/arena_bot_wasm.wasm
```

## Submit

Register for a token, then upload the `.wasm`:

```sh
TOKEN=$(curl -s -X POST http://localhost:3000/register \
  -H 'Content-Type: application/json' \
  -d '{"password":"arena","team":"team-wasm"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')

curl -X POST http://localhost:3000/bots \
  -H "Authorization: Bearer $TOKEN" \
  --data-binary @target/wasm32-unknown-unknown/release/arena_bot_wasm.wasm
```

The Arena instantiates the module, calls `init` with the tick-0 observation,
then calls `tick` each tick within a fuel budget. Re-uploading replaces it.
