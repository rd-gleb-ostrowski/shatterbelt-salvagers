# Connection resolver & Default Bot priority

Status: ready-for-agent
Type: AFK
User stories: 8, 9

## Parent

`.scratch/arena-server/PRD.md`

## What to build

Decide who actually drives each ship so every match slot always plays (ADR-0001). End-to-end across
all three bot kinds:

- For each team, resolve the controller by **priority: WS Bot → WASM Bot → Default Bot** — a live
  connection supersedes an upload, which supersedes the built-in fallback.
- A slot with no submitted bot at all is driven by the **Default Bot** so the field is always full.
- Resolution reflects current state: e.g. a team with an uploaded WASM Bot that then connects a WS
  Bot is driven by the WS Bot.

This stitches the WS path, the wasmtime host, and the Default Bot into a single per-slot decision.

## Acceptance criteria

- [ ] Every match slot is assigned a controller and a full field always plays.
- [ ] A team with a live WS Bot is driven by it even if a WASM Bot is also uploaded.
- [ ] A team with only a WASM Bot is driven by the wasmtime host.
- [ ] A team with neither is driven by the Default Bot.
- [ ] Resolution is tested for each priority combination via stubbed transport/bots.

## Blocked by

- `03-ws-bot-connect-play.md`
- `05-wasmtime-host.md`
