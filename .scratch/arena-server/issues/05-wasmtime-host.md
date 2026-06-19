# wasmtime host (ABI, fuel, log)

Status: ready-for-agent
Type: AFK
User stories: 7, 21

## Parent

`.scratch/arena-server/PRD.md`

## What to build

The execution half of the WASM path: an in-process **wasmtime** host that runs an uploaded WASM Bot
against the same JSON contract as WS Bots (ADR-0004), so it plays headless and fast.

- Instantiate the stored `.wasm` via the core-wasm ABI: `memory` / `alloc` / `init(tick-0 obs)` /
  `tick` returning a packed ptr/len intent. The instance **persists per match**.
- **Warm-up**: call `init` with the tick-0 observation before the match begins.
- Per-tick **fuel** budget: exceeding it yields "no action this tick" (previous intent persists)
  rather than stalling the match.
- Optional `log` host import the bot can call, surfaced for the Admin to read.

The host consumes the artifact stored by the upload slice and feeds the same observation/intent
JSON the engine and WS path use.

## Acceptance criteria

- [ ] A stored `.wasm` is instantiated, `init` is called with the tick-0 observation (warm-up), and
      `tick` is called each tick with the instance persisting across the match.
- [ ] `alloc`/`memory` round-trip observation in and packed ptr/len intent out per the ABI.
- [ ] A tick that exceeds the fuel budget yields "no action this tick" and the previous intent
      persists; the match continues.
- [ ] A `log` import invoked by the bot is captured and exposed for the Admin.
- [ ] WASM-bot seam test: a tiny known `.wasm` bot runs a headless match and demonstrably acts,
      proving init/tick/alloc, fuel limiting, and warm-up.

## Blocked by

- `01-server-skeleton-live-loop.md`
- `04-wasm-bot-upload.md`
