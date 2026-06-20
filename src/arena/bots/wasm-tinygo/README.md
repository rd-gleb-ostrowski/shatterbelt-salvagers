# Minimal WASM Bot — TinyGo

A bare-minimal, no-abstraction core-wasm ABI reference (ADR-0004 /
[`PROTOCOL.md`](../../PROTOCOL.md) §9) written in Go, compiled with
[TinyGo](https://tinygo.org). `main.go` exports the ABI by hand — you see every
pointer.

## What it shows

The module exports:

- `memory` — linear memory (exported automatically).
- `alloc(len) -> ptr` — the host asks for a buffer and writes observation JSON in.
- `init(ptr, len)` — called once before the match with the tick-0 observation.
- `tick(ptr, len) -> i64` — read the observation at `[ptr, len)`, write an action
  JSON, return the packed `(out_ptr << 32) | out_len`.

Memory is accessed directly with `unsafe`; the standard `encoding/json` parses
the observation into typed structs and marshals the action. The exports use
`//go:wasmexport`. The decision is a trivial placeholder (steer at the nearest
relic, thrust, fire).

> TinyGo emits a WASI "reactor" module: runtime/GC/global setup runs in an
> exported `_initialize` that the Arena host calls once after instantiation. The
> module declares no host imports (`-target=wasm-unknown`).

## Build

Needs Go and TinyGo (0.39+).

```sh
mkdir -p build
tinygo build -target=wasm-unknown -o build/bot.wasm .
```

## Submit

Register for a token, then upload the `.wasm`:

```sh
TOKEN=$(curl -s -X POST http://localhost:3000/register \
  -H 'Content-Type: application/json' \
  -d '{"password":"arena","team":"team-tinygo"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')

curl -X POST http://localhost:3000/bots \
  -H "Authorization: Bearer $TOKEN" \
  --data-binary @build/bot.wasm
```

The Arena instantiates the module, calls `_initialize` then `init` with the
tick-0 observation, then calls `tick` each tick within a fuel budget.
Re-uploading replaces it.
