# Headless-fast match runner

Status: ready-for-agent
Type: AFK
User stories: 13, 23

## Parent

`.scratch/arena-server/PRD.md`

## What to build

Run a whole ladder's worth of matches in seconds by removing the network from the loop.

- A background runner that executes **headless-fast** matches **uncapped** (no ~30 Hz pacing, no
  deadlines), driven by **WASM Bots and Default Bots** via the wasmtime host and resolver.
- **WS Bots are restricted to live matches** and are excluded from headless-fast runs so the
  network never bottlenecks an uncapped run (a WS team falls back to its WASM/Default per the
  resolver in headless context).
- Headless matches run continuously in the background so there's a steady stream of finished
  matches (to feed the ladder in the next slice).

## Acceptance criteria

- [ ] The server runs headless matches uncapped (no live pacing/deadlines), much faster than
      real-time.
- [ ] Headless matches are driven by WASM/Default bots; WS Bots cannot participate in headless runs.
- [ ] In a headless run, a team that only has a WS connection is filled per the resolver
      (WASM/Default), not the WS Bot.
- [ ] Headless matches run continuously in the background and each produces a finished result with a
      full ranking.
- [ ] Headless running is verifiable via a tiny known WASM bot completing a fast match.

## Blocked by

- `05-wasmtime-host.md`
- `06-connection-resolver.md`
