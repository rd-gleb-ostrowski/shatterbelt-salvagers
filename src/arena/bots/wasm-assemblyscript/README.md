# Minimal WASM Bot — AssemblyScript

A bare-minimal, no-abstraction core-wasm ABI reference (ADR-0004 /
[`PROTOCOL.md`](../../PROTOCOL.md) §9) written in AssemblyScript. `assembly/bot.ts`
exports the ABI by hand — you see every pointer.

## What it shows

The module exports exactly:

- `memory` — linear memory (exported automatically).
- `alloc(len) -> ptr` — the host asks for a buffer and writes observation JSON in.
- `init(ptr, len)` — called once before the match with the tick-0 observation.
- `tick(ptr, len) -> i64` — read the observation at `[ptr, len)`, write an action
  JSON, return the packed `(out_ptr << 32) | out_len`.

JSON is parsed with the `assemblyscript-json` library (AssemblyScript has no
built-in JSON); the ABI plumbing — `String.UTF8.decodeUnsafe`, `heap.alloc`,
`memory.copy`, the packed return — is explicit. The decision is a trivial
placeholder (steer at the nearest relic, thrust, fire).

## Build

```sh
npm install
npm run build      # asc ... --runtime stub  ->  build/bot.wasm
```

The `stub` runtime keeps the module tiny and host-friendly. The Arena funds
instantiation fuel, so AssemblyScript's `start` section runs fine.

## Submit

Register for a token, then upload the `.wasm`:

```sh
TOKEN=$(curl -s -X POST http://localhost:3000/register \
  -H 'Content-Type: application/json' \
  -d '{"password":"arena","team":"team-as"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')

curl -X POST http://localhost:3000/bots \
  -H "Authorization: Bearer $TOKEN" \
  --data-binary @build/bot.wasm
```

The Arena instantiates the module, calls `init` with the tick-0 observation,
then calls `tick` each tick within a fuel budget. Re-uploading replaces it.
